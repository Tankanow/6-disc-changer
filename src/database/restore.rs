use crate::database::{
    DatabaseError, DatabaseResult, restoration_status::SharedRestorationStatus,
    storage::StorageProvider,
};
use sqlx::{Connection, Row, SqliteConnection};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tracing::{error, info, warn};

/// Manages database restoration operations
pub struct RestorationChecker {
    database_path: PathBuf,
    storage: Arc<dyn StorageProvider>,
    environment: String,
    server_id: Option<String>,
    status: SharedRestorationStatus,
}

impl RestorationChecker {
    /// Create a new RestorationChecker instance
    pub fn new(
        database_path: PathBuf,
        storage: Arc<dyn StorageProvider>,
        environment: &str,
        server_id: Option<&str>,
        status: SharedRestorationStatus,
    ) -> Self {
        Self {
            database_path,
            storage,
            environment: environment.to_string(),
            server_id: server_id.map(|s| s.to_string()),
            status,
        }
    }

    /// Check if database restoration is needed
    pub async fn is_restoration_needed(&self) -> DatabaseResult<bool> {
        info!("Checking if database restoration is needed");

        // Check if database file exists
        if !self.database_path.exists() {
            info!(
                "Database file does not exist at {:?}, restoration needed",
                self.database_path
            );
            return Ok(true);
        }

        // Check if database file is empty
        let metadata = fs::metadata(&self.database_path).await.map_err(|e| {
            DatabaseError::FileSystem(format!("Failed to read database metadata: {}", e))
        })?;

        if metadata.len() == 0 {
            info!("Database file is empty, restoration needed");
            return Ok(true);
        }

        // Check if database can be opened and is valid
        match self.verify_database_integrity().await {
            Ok(true) => {
                info!("Database integrity verified, no restoration needed");
                Ok(false)
            }
            Ok(false) => {
                warn!("Database integrity check failed, restoration needed");
                Ok(true)
            }
            Err(e) => {
                error!(
                    "Error verifying database integrity: {}, restoration needed",
                    e
                );
                Ok(true)
            }
        }
    }

    /// Verify database integrity by attempting to connect and run a simple query
    async fn verify_database_integrity(&self) -> DatabaseResult<bool> {
        let db_url = format!("sqlite:{}", self.database_path.display());

        // Try to open a connection
        let mut conn = match SqliteConnection::connect(&db_url).await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("Failed to connect to database: {}", e);
                return Ok(false);
            }
        };

        // Run integrity check
        match sqlx::query("PRAGMA integrity_check")
            .fetch_one(&mut conn)
            .await
        {
            Ok(row) => {
                let result: String = row.try_get(0).unwrap_or_else(|_| "error".to_string());
                if result == "ok" {
                    // Additional check: verify we can query the users table
                    match sqlx::query("SELECT COUNT(*) FROM users")
                        .fetch_one(&mut conn)
                        .await
                    {
                        Ok(_) => {
                            info!("Database integrity check passed");
                            Ok(true)
                        }
                        Err(e) => {
                            warn!("Failed to query users table: {}", e);
                            Ok(false)
                        }
                    }
                } else {
                    warn!("Database integrity check returned: {}", result);
                    Ok(false)
                }
            }
            Err(e) => {
                warn!("Failed to run integrity check: {}", e);
                Ok(false)
            }
        }
    }

    /// Find the latest backup available in storage
    pub async fn find_latest_backup(&self) -> DatabaseResult<Option<String>> {
        info!("Searching for latest backup in storage");

        let backups = self.storage.list_backups().await?;

        if backups.is_empty() {
            warn!("No backups found in storage");
            return Ok(None);
        }

        // Parse and sort backups by timestamp
        let mut parsed_backups: Vec<(String, chrono::DateTime<chrono::Utc>)> = Vec::new();

        for backup in backups {
            // Parse backup ID to extract timestamp
            if let Some(backup_id) = crate::database::backup_naming::BackupId::parse(&backup) {
                parsed_backups.push((backup, backup_id.timestamp().clone()));
            }
        }

        if parsed_backups.is_empty() {
            warn!("No valid backups found with parseable timestamps");
            return Ok(None);
        }

        // Sort by timestamp (newest first)
        parsed_backups.sort_by(|a, b| b.1.cmp(&a.1));

        let latest = &parsed_backups[0].0;
        info!("Found latest backup: {}", latest);

        Ok(Some(latest.to_string()))
    }

    /// Restore database from a backup
    pub async fn restore_from_backup(&self, backup_name: &str) -> DatabaseResult<()> {
        info!("Starting database restoration from: {}", backup_name);

        // Update status
        {
            let mut status = self.status.lock().unwrap();
            status.start_restoration();
        }

        // Ensure parent directory exists
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                DatabaseError::FileSystem(format!("Failed to create database directory: {}", e))
            })?;
        }

        // Create a temporary file for restoration
        let temp_path = self.database_path.with_extension("restore.tmp");

        // Download backup to temporary file
        match self.download_backup(backup_name, &temp_path).await {
            Ok(size) => {
                info!("Successfully downloaded backup: {} bytes", size);

                // Verify the downloaded database
                if self.verify_restored_database(&temp_path).await? {
                    // Move temporary file to final location
                    fs::rename(&temp_path, &self.database_path)
                        .await
                        .map_err(|e| {
                            DatabaseError::FileSystem(format!(
                                "Failed to move restored database: {}",
                                e
                            ))
                        })?;

                    // Update status
                    {
                        let mut status = self.status.lock().unwrap();
                        status.complete_restoration(
                            true,
                            None,
                            Some(size as u64),
                            backup_name.to_string(),
                        );
                    }

                    info!("Database restoration completed successfully");
                    Ok(())
                } else {
                    // Clean up temporary file
                    let _ = fs::remove_file(&temp_path).await;

                    let error = "Restored database failed integrity check";
                    {
                        let mut status = self.status.lock().unwrap();
                        status.complete_restoration(
                            false,
                            Some(error.to_string()),
                            None,
                            backup_name.to_string(),
                        );
                    }

                    Err(DatabaseError::Restoration(error.to_string()))
                }
            }
            Err(e) => {
                // Clean up temporary file if it exists
                let _ = fs::remove_file(&temp_path).await;

                // Update status
                {
                    let mut status = self.status.lock().unwrap();
                    status.complete_restoration(
                        false,
                        Some(e.to_string()),
                        None,
                        backup_name.to_string(),
                    );
                }

                Err(e)
            }
        }
    }

    /// Download backup from storage to a local file
    async fn download_backup(&self, backup_name: &str, dest_path: &Path) -> DatabaseResult<usize> {
        info!("Downloading backup {} to {:?}", backup_name, dest_path);

        // Create parent directory if needed
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                DatabaseError::FileSystem(format!("Failed to create parent directory: {}", e))
            })?;
        }

        // Read backup data from storage
        let data = self.storage.read_backup(backup_name).await?;
        let size = data.len();

        // Write to destination
        fs::write(dest_path, data).await.map_err(|e| {
            DatabaseError::FileSystem(format!("Failed to write backup to disk: {}", e))
        })?;

        Ok(size)
    }

    /// Verify a restored database file
    async fn verify_restored_database(&self, db_path: &Path) -> DatabaseResult<bool> {
        let db_url = format!("sqlite:{}", db_path.display());

        // Try to open a connection
        let mut conn = match SqliteConnection::connect(&db_url).await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("Failed to connect to restored database: {}", e);
                return Ok(false);
            }
        };

        // Run integrity check
        match sqlx::query("PRAGMA integrity_check")
            .fetch_one(&mut conn)
            .await
        {
            Ok(row) => {
                let result: String = row.try_get(0).unwrap_or_else(|_| "error".to_string());
                Ok(result == "ok")
            }
            Err(e) => {
                warn!("Failed to run integrity check on restored database: {}", e);
                Ok(false)
            }
        }
    }

    /// Check if restoration is needed and perform it if necessary
    pub async fn check_and_restore_if_needed(&self) -> DatabaseResult<bool> {
        if !self.is_restoration_needed().await? {
            info!("Database restoration not needed");
            return Ok(false);
        }

        info!("Database restoration needed, searching for latest backup");

        match self.find_latest_backup().await? {
            Some(backup_name) => {
                info!("Found backup to restore: {}", backup_name);
                self.restore_from_backup(&backup_name).await?;
                Ok(true)
            }
            None => {
                warn!("No backups available for restoration");
                // Let the application continue without restoration
                // The database will be created fresh by migrations
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{
        restoration_status::create_shared_restoration_status,
        storage::local_storage::LocalStorageProvider,
    };
    use tempfile::TempDir;

    async fn create_test_restoration_checker() -> (RestorationChecker, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let backup_dir = temp_dir.path().join("backups");

        let config = crate::config::BackupConfig {
            local_backup_dir: backup_dir.clone(),
            ..Default::default()
        };
        let storage = Arc::new(LocalStorageProvider::new(&config));
        let status = create_shared_restoration_status();

        let checker = RestorationChecker::new(db_path, storage, "test", None, status);

        (checker, temp_dir)
    }

    #[tokio::test]
    async fn test_restoration_needed_no_database() {
        let (checker, _temp_dir) = create_test_restoration_checker().await;

        let needed = checker.is_restoration_needed().await.unwrap();
        assert!(
            needed,
            "Should need restoration when database doesn't exist"
        );
    }

    #[tokio::test]
    async fn test_restoration_needed_empty_database() {
        let (checker, _temp_dir) = create_test_restoration_checker().await;

        // Create empty database file
        fs::write(&checker.database_path, b"").await.unwrap();

        let needed = checker.is_restoration_needed().await.unwrap();
        assert!(needed, "Should need restoration when database is empty");
    }

    #[tokio::test]
    async fn test_find_latest_backup_no_backups() {
        let (checker, _temp_dir) = create_test_restoration_checker().await;

        let latest = checker.find_latest_backup().await.unwrap();
        assert!(latest.is_none(), "Should return None when no backups exist");
    }

    #[tokio::test]
    async fn test_find_latest_backup_with_backups() {
        let (checker, temp_dir) = create_test_restoration_checker().await;
        let backup_dir = temp_dir.path().join("backups");

        // Create backup directory
        fs::create_dir_all(&backup_dir).await.unwrap();

        // Create environment directory
        let env_dir = backup_dir.join("test");
        fs::create_dir_all(&env_dir).await.unwrap();

        // Create some test backup files with proper naming format
        let backup1 = "backup_2023-01-01_100000_test_scheduled_abc123";
        let backup2 = "backup_2023-01-02_100000_test_scheduled_def456";
        let backup3 = "backup_2023-01-01_150000_test_scheduled_ghi789";

        // Files must be in the format expected by LocalStorageProvider
        fs::write(env_dir.join(format!("backup-{}.db", backup1)), b"backup1")
            .await
            .unwrap();
        fs::write(env_dir.join(format!("backup-{}.db", backup2)), b"backup2")
            .await
            .unwrap();
        fs::write(env_dir.join(format!("backup-{}.db", backup3)), b"backup3")
            .await
            .unwrap();

        let latest = checker.find_latest_backup().await.unwrap();
        assert_eq!(latest, Some(backup2.to_string()));
    }
}
