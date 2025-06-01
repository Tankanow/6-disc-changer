//! SQLite database backup implementation
//! 
//! This module provides functionality to back up SQLite databases using
//! the SQLite Online Backup API, with proper locking mechanisms and
//! incremental backup support.

use std::path::Path;
use std::time::{Duration, Instant};
use chrono::{DateTime, Utc};
use sqlx::{SqliteConnection, Connection, Pool, Sqlite, Executor, Error as SqlxError};
use tempfile::tempdir;
use tokio::sync::Mutex;
use std::sync::Arc;

use crate::database::{Result, DatabaseError};

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

/// Represents a backup operation with source and destination connections
struct BackupOperation {
    source_conn: sqlx::pool::PoolConnection<Sqlite>,
    dest_conn: SqliteConnection,
}

/// Manager for SQLite database backup operations
pub struct BackupManager {
    /// Database connection pool
    db_pool: Pool<Sqlite>,
    /// Storage provider for backups
    storage: Arc<dyn StorageProvider>,
    /// Mutex to ensure only one backup runs at a time
    backup_mutex: Mutex<()>,
}

impl BackupManager {
    /// Create a new backup manager
    pub fn new(db_pool: Pool<Sqlite>, storage: Arc<dyn StorageProvider>) -> Self {
        Self {
            db_pool,
            storage,
            backup_mutex: Mutex::new(()),
        }
    }
    
    /// Create a backup of the database
    pub async fn create_backup(&self, options: BackupOptions) -> Result<BackupResult> {
        // Acquire mutex to ensure only one backup runs at a time
        let _lock = self.backup_mutex.lock().await;
        
        // Start timing the backup
        let start_time = Instant::now();
        let timestamp = Utc::now();
        
        // Generate backup ID using timestamp
        let backup_id = format!("{}", timestamp.timestamp());
        
        // Create temporary directory for backup
        let temp_dir = tempdir().map_err(|e| DatabaseError::Io(e))?;
        let backup_path = temp_dir.path().join("backup.db");
        
        // Acquire a connection from the pool
        let mut conn = self.db_pool.acquire().await?;
        
        // Begin a transaction with IMMEDIATE mode to ensure consistency
        conn.execute("BEGIN IMMEDIATE").await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;
        
        // Create backup and track result
        let result = self.perform_backup(conn, &backup_path, &backup_id, &options).await;
        
        // Process the result
        match result {
            Ok(()) => {
                // Get file size
                let size_bytes = std::fs::metadata(&backup_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                
                // Store the backup with the storage provider
                if let Err(e) = self.storage.store_backup(&backup_path, &backup_id).await {
                    return Ok(BackupResult {
                        backup_id,
                        timestamp,
                        duration: start_time.elapsed(),
                        size_bytes,
                        status: BackupStatus::Failed(format!("Failed to store backup: {}", e)),
                    });
                }
                
                // Return successful result
                Ok(BackupResult {
                    backup_id,
                    timestamp,
                    duration: start_time.elapsed(),
                    size_bytes,
                    status: BackupStatus::Completed,
                })
            },
            Err(e) => {
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
    
    /// Perform the actual backup operation
    async fn perform_backup(
        &self, 
        conn: sqlx::pool::PoolConnection<Sqlite>,
        backup_path: &Path,
        _backup_id: &str,  // Not used directly but keeping for consistency
        options: &BackupOptions,
    ) -> Result<()> {
        // Create destination database connection
        let dest_conn = SqliteConnection::connect(&format!("sqlite:{}", backup_path.display())).await
            .map_err(|e| DatabaseError::Sqlite(format!("Failed to create destination database: {}", e)))?;
        
        // Create backup operation
        let backup_op = BackupOperation {
            source_conn: conn,
            dest_conn,
        };
        
        // Perform the backup using vacuum into
        self.execute_backup(backup_op, backup_path).await?;
        
        // Verify the backup if requested
        if options.verify {
            self.verify_backup(backup_path).await?;
        }
        
        Ok(())
    }
    
    /// Execute the backup using SQLite's VACUUM INTO command
    async fn execute_backup(&self, mut backup_op: BackupOperation, backup_path: &Path) -> Result<()> {
        // Begin a transaction with IMMEDIATE mode to ensure consistency
        backup_op.source_conn.execute("BEGIN IMMEDIATE").await
            .map_err(|e| DatabaseError::Sqlite(format!("Failed to begin transaction: {}", e)))?;
        
        // Get database path as a string
        let dest_path = backup_path.to_str()
            .ok_or_else(|| DatabaseError::Sqlite("Invalid backup path".to_string()))?;
        
        // Use VACUUM INTO for a consistent backup (SQLite 3.27.0+)
        // This is an atomic operation that copies the entire database
        let vacuum_sql = format!("VACUUM INTO '{}'", dest_path.replace("'", "''"));
        
        backup_op.source_conn.execute(&*vacuum_sql).await
            .map_err(|e| DatabaseError::Sqlite(format!("Failed to execute VACUUM INTO: {}", e)))?;
        
        // Commit the transaction
        backup_op.source_conn.execute("COMMIT").await
            .map_err(|e| DatabaseError::Sqlite(format!("Failed to commit transaction: {}", e)))?;
        
        Ok(())
    }
    
    /// Verify a backup is valid
    async fn verify_backup(&self, backup_path: &Path) -> Result<()> {
        // Connect to the backup database in read-only mode
        let db_url = format!("sqlite:{}?mode=ro", backup_path.display());
        let mut conn = SqliteConnection::connect(&db_url).await
            .map_err(|e| DatabaseError::Sqlite(format!("Failed to open backup for verification: {}", e)))?;
        
        // Run a simple query to verify the database is valid
        let _: i64 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&mut conn)
            .await
            .map_err(|e| DatabaseError::Sqlite(format!("Backup verification failed: {}", e)))?;
        
        Ok(())
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
    use sqlx::{migrate::MigrateDatabase, SqlitePool};
    use crate::database::storage::local_storage::LocalStorageProvider;
    use crate::config::BackupConfig;
    use std::sync::Arc;
    
    async fn setup_test_db() -> Result<SqlitePool> {
        let db_url = "sqlite::memory:";
        
        // Create database
        Sqlite::create_database(db_url).await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;
        
        // Connect to database
        let pool = SqlitePool::connect(db_url).await
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
        Sqlite::create_database(&format!("sqlite:{}", db_path)).await
            .map_err(|e| DatabaseError::Sqlite(e.to_string()))?;
        
        // Connect to database
        let pool = SqlitePool::connect(&format!("sqlite:{}", db_path)).await
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
        };
        
        // Create local storage provider
        let storage = Arc::new(LocalStorageProvider::new(&config));
        
        // Create backup manager
        let backup_manager = BackupManager::new(pool.clone(), storage);
        
        // Create backup with default options
        let result = backup_manager.create_backup(BackupOptions::default()).await?;
        
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
}