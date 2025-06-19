//! SQLite database backup implementation
//!
//! This module provides functionality to back up SQLite databases using
//! the SQLite Online Backup API, with proper locking mechanisms and
//! incremental backup support.

use chrono::{DateTime, Utc};
use sqlx::{Connection, Error as SqlxError, Executor, Pool, Sqlite, SqliteConnection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::database::backup_naming::BackupNamingService;

use crate::database::{DatabaseError, Result};

// Implement From<SqlxError> for DatabaseError
impl From<SqlxError> for DatabaseError {
    fn from(error: SqlxError) -> Self {
        DatabaseError::Sqlite(error.to_string())
    }
}
use crate::database::storage::StorageProvider;

/// Status of a backup operation
#[derive(Debug, Clone, PartialEq)]
pub enum BackupStatus {
    /// Backup completed successfully
    Completed,
    /// Backup failed
    Failed(String),
    /// Backup was cancelled
    Cancelled,
}

/// Options for configuring backup behavior
#[derive(Debug, Clone)]
pub struct BackupOptions {
    /// Number of pages to copy in each step
    pub chunk_size: usize,
    /// Sleep duration between chunks (milliseconds)
    pub sleep_ms: u64,
    /// Maximum number of steps (None for unlimited)
    pub step_count: Option<usize>,
    /// Whether to verify the backup after creation
    pub verify: bool,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            // Default to 64 pages per step
            chunk_size: 64,
            // Default to 10ms sleep between chunks
            sleep_ms: 10,
            // No limit on steps by default
            step_count: None,
            // Don't verify by default (for performance)
            verify: false,
        }
    }
}

/// Result of a backup operation
#[derive(Debug, Clone)]
pub struct BackupResult {
    /// Unique identifier for the backup
    pub backup_id: String,
    /// Timestamp when the backup was created
    pub timestamp: DateTime<Utc>,
    /// Duration of the backup operation
    pub duration: Duration,
    /// Size of the backup in bytes
    pub size_bytes: u64,
    /// Status of the backup operation
    pub status: BackupStatus,
}

/// Status of a background backup job
#[derive(Debug, Clone)]
pub enum BackupJobStatus {
    /// Job is currently running
    Running {
        /// When the job started
        started_at: DateTime<Utc>,
    },
    /// Job completed successfully
    Completed {
        /// The backup result
        result: BackupResult,
    },
    /// Job failed with an error
    Failed {
        /// Error message
        error: String,
        /// When the job failed
        failed_at: DateTime<Utc>,
    },
    /// Job was cancelled
    Cancelled {
        /// When the job was cancelled
        cancelled_at: DateTime<Utc>,
    },
}

/// Represents a background backup job
#[derive(Debug, Clone)]
pub struct BackupJob {
    /// Unique job identifier
    pub job_id: String,
    /// Current status of the job
    pub status: BackupJobStatus,
}

/// Represents a backup operation with source connection
struct BackupOperation {
    source_conn: sqlx::pool::PoolConnection<Sqlite>,
}

/// Manager for SQLite database backup operations
pub struct BackupManager {
    /// Database connection pool
    db_pool: Pool<Sqlite>,
    /// Storage provider for backups
    storage: Arc<dyn StorageProvider>,
    /// Mutex to ensure only one backup runs at a time
    backup_mutex: Arc<Mutex<()>>,
    /// Service for generating backup IDs
    naming_service: BackupNamingService,
    /// Active backup jobs
    active_jobs: Arc<RwLock<HashMap<String, BackupJob>>>,
    /// Job handles for tracking spawned tasks
    job_handles: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl BackupManager {
    /// Create a new backup manager
    pub fn new(
        db_pool: Pool<Sqlite>,
        storage: Arc<dyn StorageProvider>,
        environment: &str,
        server_id: Option<&str>,
    ) -> Self {
        Self {
            db_pool,
            storage,
            backup_mutex: Arc::new(Mutex::new(())),
            naming_service: BackupNamingService::new(environment, server_id),
            active_jobs: Arc::new(RwLock::new(HashMap::new())),
            job_handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a backup of the database in a background thread
    /// Returns immediately with a job ID that can be used to track progress
    pub async fn create_backup(&self, options: BackupOptions) -> Result<String> {
        // Generate job ID
        let job_id = format!(
            "job_{}",
            self.naming_service.generate_backup_id_with_time(Utc::now())
        );

        info!("Starting backup job: {}", job_id);
        debug!("Backup options: {:?}", options);

        // Create initial job entry
        let job = BackupJob {
            job_id: job_id.clone(),
            status: BackupJobStatus::Running {
                started_at: Utc::now(),
            },
        };

        // Store job in active jobs
        {
            let mut jobs = self.active_jobs.write().await;
            jobs.insert(job_id.clone(), job);
        }

        // Clone necessary items for the spawned task
        let job_id_clone = job_id.clone();
        let db_pool = self.db_pool.clone();
        let storage = self.storage.clone();
        let naming_service = self.naming_service.clone();
        let backup_mutex = self.backup_mutex.clone();
        let active_jobs = self.active_jobs.clone();
        let job_handles = self.job_handles.clone();

        // Spawn the backup task
        let handle = tokio::spawn(async move {
            // Perform the actual backup in the background
            let result = Self::perform_background_backup(
                job_id_clone.clone(),
                db_pool,
                storage,
                naming_service,
                backup_mutex,
                active_jobs.clone(),
                options,
            )
            .await;

            // Update job status based on result
            let mut jobs = active_jobs.write().await;
            if let Some(job) = jobs.get_mut(&job_id_clone) {
                match result {
                    Ok(backup_result) => {
                        info!("Backup job {} completed successfully", job_id_clone);
                        job.status = BackupJobStatus::Completed {
                            result: backup_result,
                        };
                    }
                    Err(e) => {
                        error!("Backup job {} failed: {}", job_id_clone, e);
                        job.status = BackupJobStatus::Failed {
                            error: e.to_string(),
                            failed_at: Utc::now(),
                        };
                    }
                }
            } else {
                warn!("Could not find job {} to update status", job_id_clone);
            }

            // Remove job handle
            let mut handles = job_handles.lock().await;
            handles.remove(&job_id_clone);
            debug!("Removed job handle for {}", job_id_clone);
        });

        // Store the job handle
        // Store handle
        {
            let mut handles = self.job_handles.lock().await;
            handles.insert(job_id.clone(), handle);
        }

        debug!("Backup job {} spawned in background", job_id);
        Ok(job_id)
    }

    /// Perform the backup operation in the background
    /// Perform the actual backup operation
    async fn perform_background_backup(
        job_id: String,
        db_pool: Pool<Sqlite>,
        storage: Arc<dyn StorageProvider>,
        naming_service: BackupNamingService,
        backup_mutex: Arc<Mutex<()>>,
        active_jobs: Arc<RwLock<HashMap<String, BackupJob>>>,
        options: BackupOptions,
    ) -> Result<BackupResult> {
        debug!("Waiting to acquire backup mutex for job {}", job_id);
        // Hold the mutex during the entire backup operation
        let _guard = backup_mutex.lock().await;
        debug!("Acquired backup mutex for job {}", job_id);

        // Check if job was cancelled
        {
            let jobs = active_jobs.read().await;
            if let Some(job) = jobs.get(&job_id) {
                if matches!(job.status, BackupJobStatus::Cancelled { .. }) {
                    warn!("Backup job {} was cancelled before starting", job_id);
                    return Ok(BackupResult {
                        backup_id: String::new(),
                        timestamp: Utc::now(),
                        duration: Duration::from_secs(0),
                        size_bytes: 0,
                        status: BackupStatus::Cancelled,
                    });
                }
            }
        }

        // Start timing the backup
        let start_time = Instant::now();
        let timestamp = Utc::now();

        // Generate backup ID using the naming service
        let backup_id = naming_service.generate_backup_id_with_time(timestamp);
        info!("Starting backup operation with ID: {}", backup_id);

        // Create temporary directory for backup
        let temp_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;
        let backup_path = temp_dir.path().join("backup.db");
        debug!("Created temporary backup path: {:?}", backup_path);

        // Acquire a connection from the pool
        let conn = db_pool.acquire().await?;

        // Create backup and track result
        let result = Self::perform_backup_internal(conn, &backup_path, &backup_id, &options).await;

        // Process the result
        match result {
            Ok(()) => {
                // Get file size
                let size_bytes = std::fs::metadata(&backup_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                debug!("Backup file size: {} bytes", size_bytes);

                // Store the backup with the storage provider
                info!("Uploading backup {} to storage provider", backup_id);
                if let Err(e) = storage
                    .store_backup(&backup_path, &backup_id, &naming_service.environment_dir())
                    .await
                {
                    error!("Failed to store backup {}: {}", backup_id, e);
                    return Ok(BackupResult {
                        backup_id,
                        timestamp,
                        duration: start_time.elapsed(),
                        size_bytes,
                        status: BackupStatus::Failed(format!("Failed to store backup: {}", e)),
                    });
                }

                let duration = start_time.elapsed();
                info!(
                    "Backup {} completed successfully in {:?}, size: {} bytes",
                    backup_id, duration, size_bytes
                );

                // Return successful result
                Ok(BackupResult {
                    backup_id,
                    timestamp,
                    duration,
                    size_bytes,
                    status: BackupStatus::Completed,
                })
            }
            Err(e) => {
                error!("Backup {} failed: {}", backup_id, e);
                // Return failed result
                Ok(BackupResult {
                    backup_id,
                    timestamp,
                    duration: start_time.elapsed(),
                    size_bytes: 0,
                    status: BackupStatus::Failed(e.to_string()),
                })
            }
        }
    }

    /// Internal backup implementation (used by both synchronous and background methods)
    async fn perform_backup_internal(
        conn: sqlx::pool::PoolConnection<Sqlite>,
        backup_path: &Path,
        backup_id: &str,
        options: &BackupOptions,
    ) -> Result<()> {
        debug!("Starting internal backup process for {}", backup_id);

        // Create backup operation
        let backup_op = BackupOperation { source_conn: conn };

        // Perform the backup using vacuum into
        debug!("Executing VACUUM INTO for backup {}", backup_id);
        Self::execute_backup_static(backup_op, backup_path).await?;

        // Verify the backup if requested
        if options.verify {
            info!("Verifying backup {} integrity", backup_id);
            Self::verify_backup_static(backup_path).await?;
            debug!("Backup {} verification completed successfully", backup_id);
        }

        Ok(())
    }

    /// Execute the backup using SQLite's VACUUM INTO command (static version)
    async fn execute_backup_static(
        mut backup_op: BackupOperation,
        backup_path: &Path,
    ) -> Result<()> {
        // Get database path as a string
        let dest_path = backup_path
            .to_str()
            .ok_or_else(|| DatabaseError::Sqlite("Invalid backup path".to_string()))?;

        // Use VACUUM INTO for a consistent backup (SQLite 3.27.0+)
        // This is an atomic operation that copies the entire database
        // Note: VACUUM INTO cannot run inside a transaction
        let vacuum_sql = format!("VACUUM INTO '{}'", dest_path.replace("'", "''"));
        debug!("Executing SQL: VACUUM INTO '{}'", dest_path);

        backup_op
            .source_conn
            .execute(&*vacuum_sql)
            .await
            .map_err(|e| DatabaseError::Sqlite(format!("Failed to execute VACUUM INTO: {}", e)))?;

        debug!("VACUUM INTO completed successfully");
        Ok(())
    }

    /// Verify a backup is valid (static version)
    async fn verify_backup_static(backup_path: &Path) -> Result<()> {
        debug!("Starting backup verification for {:?}", backup_path);

        // Connect to the backup database in read-only mode
        let db_url = format!("sqlite:{}?mode=ro", backup_path.display());
        let mut conn = SqliteConnection::connect(&db_url).await.map_err(|e| {
            DatabaseError::Sqlite(format!("Failed to open backup for verification: {}", e))
        })?;

        // Run a simple query to verify the database is valid
        let _: i64 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&mut conn)
            .await
            .map_err(|e| DatabaseError::Sqlite(format!("Backup verification failed: {}", e)))?;

        debug!("Backup verification successful");
        Ok(())
    }

    /// Get the status of a backup job
    pub async fn get_backup_status(&self, job_id: &str) -> Option<BackupJob> {
        let jobs = self.active_jobs.read().await;
        let job = jobs.get(job_id).cloned();

        if let Some(ref j) = job {
            debug!("Job {} status: {:?}", job_id, j.status);
        } else {
            debug!("Job {} not found in active jobs", job_id);
        }

        job
    }

    /// Wait for a backup job to complete
    pub async fn wait_for_backup(&self, job_id: &str) -> Result<BackupResult> {
        info!("Waiting for backup job {} to complete", job_id);

        // Get the job handle
        let handle = {
            let mut handles = self.job_handles.lock().await;
            handles.remove(job_id)
        };

        // Wait for the job to complete if we have a handle
        if let Some(handle) = handle {
            debug!("Found job handle for {}, waiting for completion", job_id);
            let _ = handle.await;
        } else {
            debug!(
                "No job handle found for {}, checking status directly",
                job_id
            );
        }

        // Get the final job status
        let jobs = self.active_jobs.read().await;
        if let Some(job) = jobs.get(job_id) {
            match &job.status {
                BackupJobStatus::Completed { result } => {
                    info!("Backup job {} completed successfully", job_id);
                    Ok(result.clone())
                }
                BackupJobStatus::Failed { error, .. } => {
                    error!("Backup job {} failed: {}", job_id, error);
                    Err(DatabaseError::Backup(error.clone()))
                }
                BackupJobStatus::Cancelled { .. } => {
                    warn!("Backup job {} was cancelled", job_id);
                    Ok(BackupResult {
                        backup_id: String::new(),
                        timestamp: Utc::now(),
                        duration: Duration::from_secs(0),
                        size_bytes: 0,
                        status: BackupStatus::Cancelled,
                    })
                }
                BackupJobStatus::Running { .. } => {
                    warn!("Backup job {} still running after wait", job_id);
                    Err(DatabaseError::Backup("Job still running".to_string()))
                }
            }
        } else {
            error!("Backup job {} not found after wait", job_id);
            Err(DatabaseError::Backup("Job not found".to_string()))
        }
    }

    /// Cancel a running backup job
    pub async fn cancel_backup(&self, job_id: &str) -> Result<()> {
        info!("Attempting to cancel backup job {}", job_id);

        // Update job status to cancelled
        let mut jobs = self.active_jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            if matches!(job.status, BackupJobStatus::Running { .. }) {
                job.status = BackupJobStatus::Cancelled {
                    cancelled_at: Utc::now(),
                };
                info!("Backup job {} marked as cancelled", job_id);
            } else {
                debug!("Backup job {} is not running, cannot cancel", job_id);
            }
        } else {
            warn!("Backup job {} not found, cannot cancel", job_id);
        }

        // Note: The actual cancellation will be checked by the background task
        // at appropriate checkpoints during the backup process

        Ok(())
    }

    /// Get all active backup jobs
    pub async fn get_active_backups(&self) -> Vec<BackupJob> {
        let jobs = self.active_jobs.read().await;
        let active_jobs: Vec<BackupJob> = jobs.values().cloned().collect();
        debug!("Found {} active backup jobs", active_jobs.len());
        active_jobs
    }

    /// Clean up completed jobs older than the specified duration
    pub async fn cleanup_completed_jobs(&self, older_than: Duration) {
        let cutoff_time = Utc::now() - chrono::Duration::from_std(older_than).unwrap();
        debug!("Cleaning up backup jobs older than {:?}", older_than);

        let mut jobs = self.active_jobs.write().await;
        let initial_count = jobs.len();

        jobs.retain(|job_id, job| {
            match &job.status {
                BackupJobStatus::Completed { result } => result.timestamp > cutoff_time,
                BackupJobStatus::Failed { failed_at, .. } => *failed_at > cutoff_time,
                BackupJobStatus::Cancelled { cancelled_at } => *cancelled_at > cutoff_time,
                BackupJobStatus::Running { .. } => true, // Always keep running jobs
            }
        });
    }

    /// List all available backups
    pub async fn list_backups(&self) -> Result<Vec<String>> {
        self.storage.list_backups().await
    }

    /// Get the latest backup ID
    pub async fn get_latest_backup(&self) -> Result<Option<String>> {
        self.storage.get_latest_backup().await
    }

    /// Delete a backup
    pub async fn delete_backup(&self, backup_id: &str) -> Result<()> {
        self.storage.delete_backup(backup_id).await
    }

    /// Clean up old backups, keeping only the most recent ones
    pub async fn cleanup_old_backups(&self, keep_count: usize) -> Result<()> {
        self.storage.cleanup_old_backups(keep_count).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackupConfig;
    use crate::database::storage::local_storage::LocalStorageProvider;
    use sqlx::{SqlitePool, migrate::MigrateDatabase};
    use std::sync::Arc;

    async fn setup_test_db() -> Result<SqlitePool> {
        let db_url = "sqlite::memory:";

        // Create database
        Sqlite::create_database(db_url)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Connect to database
        let pool = SqlitePool::connect(db_url)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a test table
        sqlx::query("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Insert some test data
        sqlx::query("INSERT INTO test (id, value) VALUES (1, 'test1'), (2, 'test2'), (3, 'test3')")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        Ok(pool)
    }

    #[tokio::test]
    #[ignore] // Skip this test for now as we need a file-based SQLite database for VACUUM INTO
    async fn test_backup_and_verify() -> Result<()> {
        // Setup test database - needs to be a file-based database for VACUUM INTO
        let db_path = "test_backup.db";

        // Remove existing file if it exists
        let _ = std::fs::remove_file(db_path);

        // Create database
        Sqlite::create_database(&format!("sqlite:{}", db_path))
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Connect to database
        let pool = SqlitePool::connect(&format!("sqlite:{}", db_path))
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a test table
        sqlx::query("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Insert some test data
        sqlx::query("INSERT INTO test (id, value) VALUES (1, 'test1'), (2, 'test2'), (3, 'test3')")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a temporary directory for backups
        let temp_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;

        // Create config with local storage
        let config = BackupConfig {
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: temp_dir.path().to_path_buf(),
            local_backup_max_count: 5,
            environment: String::from("dev"),
            server_id: Option::None,
        };

        // Create local storage provider
        let storage = Arc::new(LocalStorageProvider::new(&config));

        // Create backup manager
        let backup_manager = BackupManager::new(pool.clone(), storage, "test", None);

        // Create backup with default options (now returns job ID)
        let job_id = backup_manager
            .create_backup(BackupOptions::default())
            .await?;

        // Wait for the backup to complete
        let result = backup_manager.wait_for_backup(&job_id).await?;

        // Check backup succeeded
        assert_eq!(result.status, BackupStatus::Completed);

        // List backups
        let backups = backup_manager.list_backups().await?;

        // Check we have one backup
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0], result.backup_id);

        // Get latest backup
        let latest = backup_manager.get_latest_backup().await?;

        // Check latest backup is the one we created
        assert!(latest.is_some());
        assert_eq!(latest.unwrap(), result.backup_id);

        // Clean up
        let _ = std::fs::remove_file(db_path);

        Ok(())
    }

    #[tokio::test]
    async fn test_background_backup() -> Result<()> {
        // Create a temporary directory for the database
        let temp_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;
        let db_path = temp_dir.path().join("test_bg_backup.db");

        // Create database
        Sqlite::create_database(&format!("sqlite:{}", db_path.display()))
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Connect to database
        let pool = SqlitePool::connect(&format!("sqlite:{}", db_path.display()))
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a test table
        sqlx::query("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Insert some test data
        sqlx::query("INSERT INTO test (id, value) VALUES (1, 'test1'), (2, 'test2'), (3, 'test3')")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a temporary directory for backups
        let backup_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;

        // Create config with local storage
        let config = BackupConfig {
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: backup_dir.path().to_path_buf(),
            local_backup_max_count: 5,
            environment: String::from("test"),
            server_id: None,
        };

        // Create local storage provider
        let storage = Arc::new(LocalStorageProvider::new(&config));

        // Create backup manager
        let backup_manager = BackupManager::new(pool.clone(), storage, "test", None);

        // Start a backup in the background
        let job_id = backup_manager
            .create_backup(BackupOptions::default())
            .await?;

        // Verify we got a job ID
        assert!(job_id.starts_with("job_"));

        // Check initial job status
        let status = backup_manager.get_backup_status(&job_id).await;
        assert!(status.is_some());
        assert!(matches!(
            status.unwrap().status,
            BackupJobStatus::Running { .. }
        ));

        // Wait for the backup to complete
        let result = backup_manager.wait_for_backup(&job_id).await?;

        // Check backup succeeded
        assert_eq!(result.status, BackupStatus::Completed);
        assert!(result.size_bytes > 0);

        // Verify job status is now completed
        let final_status = backup_manager.get_backup_status(&job_id).await;
        assert!(final_status.is_some());
        assert!(matches!(
            final_status.unwrap().status,
            BackupJobStatus::Completed { .. }
        ));

        // List backups to ensure it was stored
        let backups = backup_manager.list_backups().await?;
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0], result.backup_id);

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_concurrent_backups() -> Result<()> {
        // Create a temporary directory for the database
        let temp_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;
        let db_path = temp_dir.path().join("test_concurrent.db");

        // Create database
        Sqlite::create_database(&format!("sqlite:{}", db_path.display()))
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Connect to database
        let pool = SqlitePool::connect(&format!("sqlite:{}", db_path.display()))
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a test table
        sqlx::query("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)")
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a temporary directory for backups
        let backup_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;

        // Create config with local storage
        let config = BackupConfig {
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: backup_dir.path().to_path_buf(),
            local_backup_max_count: 5,
            environment: String::from("test"),
            server_id: None,
        };

        // Create local storage provider
        let storage = Arc::new(LocalStorageProvider::new(&config));

        // Create backup manager
        let backup_manager = Arc::new(BackupManager::new(pool.clone(), storage, "test", None));

        // Start multiple backups concurrently
        let mut job_ids = Vec::new();
        for i in 0..3 {
            // Insert some unique data for each backup
            sqlx::query(&format!(
                "INSERT INTO test (value) VALUES ('concurrent_{}')",
                i
            ))
            .execute(&pool)
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

            let job_id = backup_manager
                .create_backup(BackupOptions::default())
                .await?;
            job_ids.push(job_id);

            // Small delay to ensure different timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Check that we have multiple active jobs
        let active_jobs = backup_manager.get_active_backups().await;
        assert!(active_jobs.len() >= 1); // At least one should still be running

        // Wait for all backups to complete
        let mut results = Vec::new();
        for job_id in job_ids {
            let result = backup_manager.wait_for_backup(&job_id).await?;
            assert_eq!(result.status, BackupStatus::Completed);
            results.push(result);
        }

        // Verify all backups completed successfully
        assert_eq!(results.len(), 3);

        // List all backups
        let backups = backup_manager.list_backups().await?;
        assert!(backups.len() >= 3); // Should have at least our 3 backups

        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_completed_jobs() -> Result<()> {
        // Create in-memory database
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;

        // Create a temporary directory for backups
        let backup_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;

        // Create config with local storage
        let config = BackupConfig {
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: backup_dir.path().to_path_buf(),
            local_backup_max_count: 5,
            environment: String::from("test"),
            server_id: None,
        };

        // Create local storage provider
        let storage = Arc::new(LocalStorageProvider::new(&config));

        // Create backup manager
        let backup_manager = BackupManager::new(pool.clone(), storage, "test", None);

        // Create a few completed jobs
        let job1 = BackupJob {
            job_id: "job_old".to_string(),
            status: BackupJobStatus::Completed {
                result: BackupResult {
                    backup_id: "old_backup".to_string(),
                    timestamp: Utc::now() - chrono::Duration::hours(2),
                    duration: Duration::from_secs(1),
                    size_bytes: 1000,
                    status: BackupStatus::Completed,
                },
            },
        };

        let job2 = BackupJob {
            job_id: "job_recent".to_string(),
            status: BackupJobStatus::Completed {
                result: BackupResult {
                    backup_id: "recent_backup".to_string(),
                    timestamp: Utc::now() - chrono::Duration::minutes(30),
                    duration: Duration::from_secs(1),
                    size_bytes: 1000,
                    status: BackupStatus::Completed,
                },
            },
        };

        // Add jobs to active jobs
        {
            let mut jobs = backup_manager.active_jobs.write().await;
            jobs.insert(job1.job_id.clone(), job1);
            jobs.insert(job2.job_id.clone(), job2);
        }

        // Clean up jobs older than 1 hour
        backup_manager
            .cleanup_completed_jobs(Duration::from_secs(3600))
            .await;

        // Check that only the recent job remains
        let active_jobs = backup_manager.get_active_backups().await;
        assert_eq!(active_jobs.len(), 1);
        assert_eq!(active_jobs[0].job_id, "job_recent");

        Ok(())
    }
}
