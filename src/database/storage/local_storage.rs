//! Local filesystem implementation of the StorageProvider trait
//!
//! This module provides a local filesystem implementation of the StorageProvider
//! trait for development and fallback when AWS S3 is unavailable.

use crate::config::BackupConfig;
use crate::database::DatabaseError;
use crate::database::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as tokio_fs;
use tracing::{debug, error, info, warn};

use super::StorageProvider;

/// Provides local filesystem storage for database backups
pub struct LocalStorageProvider {
    /// Base directory for storing backups
    backup_dir: PathBuf,
    /// Maximum number of backups to keep
    max_backups: usize,
}

impl LocalStorageProvider {
    /// Create a new LocalStorageProvider with the given configuration
    pub fn new(config: &BackupConfig) -> Self {
        Self {
            backup_dir: config.local_backup_dir.clone(),
            max_backups: config.local_backup_max_count,
        }
    }

    /// Get the full path for a backup with the given ID and environment
    fn get_backup_path(&self, backup_id: &str, environment: &str) -> PathBuf {
        // Create environment subdirectory
        let env_dir = self.backup_dir.join(environment);
        env_dir.join(format!("backup-{}.db", backup_id))
    }

    /// Generate a unique backup ID based on the current timestamp
    pub fn generate_backup_id() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        format!("{}", timestamp)
    }
}

#[async_trait]
impl StorageProvider for LocalStorageProvider {
    async fn store_backup(
        &self,
        backup_path: &Path,
        backup_id: &str,
        environment: &str,
    ) -> Result<()> {
        // Ensure the environment backup directory exists
        let env_dir = self.backup_dir.join(environment);
        if !env_dir.exists() {
            debug!("Creating backup directory: {:?}", env_dir);
            tokio_fs::create_dir_all(&env_dir).await.map_err(|e| {
                error!("Failed to create backup directory {:?}: {}", env_dir, e);
                DatabaseError::Io(e)
            })?;
        }

        let dest_path = self.get_backup_path(backup_id, environment);
        info!(
            "Storing backup {} to local storage: {:?}",
            backup_id, dest_path
        );

        // Copy the backup file to the backup directory
        tokio_fs::copy(backup_path, &dest_path).await.map_err(|e| {
            error!(
                "Failed to copy backup from {:?} to {:?}: {}",
                backup_path, dest_path, e
            );
            DatabaseError::Io(e)
        })?;

        debug!(
            "Successfully stored backup {} ({} bytes)",
            backup_id,
            std::fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0)
        );
        Ok(())
    }

    async fn retrieve_backup(&self, backup_id: &str, destination_path: &Path) -> Result<()> {
        // Try to parse the environment from the backup ID
        let environment = crate::database::backup_naming::get_environment_from_backup_id(backup_id)
            .unwrap_or_else(|| {
                debug!(
                    "Could not parse environment from backup ID {}, defaulting to 'dev'",
                    backup_id
                );
                String::from("dev")
            });

        let source_path = self.get_backup_path(backup_id, &environment);
        info!(
            "Retrieving backup {} from local storage: {:?}",
            backup_id, source_path
        );

        if !source_path.exists() {
            warn!("Backup {} not found at {:?}", backup_id, source_path);
            return Err(DatabaseError::BackupNotFound);
        }

        // Create the parent directory if it doesn't exist
        if let Some(parent) = destination_path.parent() {
            if !parent.exists() {
                debug!("Creating destination directory: {:?}", parent);
                tokio_fs::create_dir_all(parent).await.map_err(|e| {
                    error!("Failed to create destination directory {:?}: {}", parent, e);
                    DatabaseError::Io(e)
                })?;
            }
        }

        // Copy the backup file to the destination
        tokio_fs::copy(&source_path, destination_path)
            .await
            .map_err(|e| {
                error!(
                    "Failed to copy backup from {:?} to {:?}: {}",
                    source_path, destination_path, e
                );
                DatabaseError::Io(e)
            })?;

        debug!(
            "Successfully retrieved backup {} ({} bytes)",
            backup_id,
            std::fs::metadata(&source_path)
                .map(|m| m.len())
                .unwrap_or(0)
        );
        Ok(())
    }

    async fn list_backups(&self) -> Result<Vec<String>> {
        debug!("Listing all backups from {:?}", self.backup_dir);

        if !self.backup_dir.exists() {
            debug!("Backup directory does not exist, returning empty list");
            return Ok(Vec::new());
        }

        let mut all_backups = Vec::new();

        // Read all environment directories
        let mut dir_entries = tokio_fs::read_dir(&self.backup_dir).await.map_err(|e| {
            error!(
                "Failed to read backup directory {:?}: {}",
                self.backup_dir, e
            );
            DatabaseError::Io(e)
        })?;

        // Iterate through environment directories
        while let Some(env_entry) = dir_entries.next_entry().await.map_err(|e| {
            error!("Failed to read directory entry: {}", e);
            DatabaseError::Io(e)
        })? {
            let env_path = env_entry.path();

            if env_path.is_dir() {
                // List backups in this environment
                let env_name = env_path.file_name().unwrap().to_str().unwrap();
                debug!("Listing backups for environment: {}", env_name);
                let env_backups = self.list_environment_backups(env_name).await?;

                debug!(
                    "Found {} backups in environment {}",
                    env_backups.len(),
                    env_name
                );
                all_backups.extend(env_backups);
            }
        }

        // Sort all backups by ID (which is timestamp-based) in descending order
        all_backups.sort_by(|a, b| b.cmp(a));

        info!("Found {} total backups", all_backups.len());
        Ok(all_backups)
    }

    async fn get_latest_backup(&self) -> Result<Option<String>> {
        let backups = self.list_backups().await?;
        Ok(backups.into_iter().next())
    }

    async fn list_environment_backups(&self, environment: &str) -> Result<Vec<String>> {
        let env_dir = self.backup_dir.join(environment);
        debug!(
            "Listing backups for environment {} in {:?}",
            environment, env_dir
        );

        if !env_dir.exists() {
            debug!("Environment directory does not exist: {:?}", env_dir);
            return Ok(Vec::new());
        }

        let mut entries = tokio_fs::read_dir(&env_dir).await.map_err(|e| {
            error!("Failed to read environment directory {:?}: {}", env_dir, e);
            DatabaseError::Io(e)
        })?;

        let mut backup_ids = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            error!("Failed to read directory entry: {}", e);
            DatabaseError::Io(e)
        })? {
            let path = entry.path();

            if path.is_file() {
                if let Some(file_name) = path.file_name() {
                    if let Some(file_name_str) = file_name.to_str() {
                        // Extract backup ID from filename (format: backup-{id}.db)
                        if file_name_str.starts_with("backup-") && file_name_str.ends_with(".db") {
                            let id = file_name_str
                                .strip_prefix("backup-")
                                .unwrap()
                                .strip_suffix(".db")
                                .unwrap();
                            backup_ids.push(id.to_string());
                        }
                    }
                }
            }
        }

        // Sort backups by ID (which is timestamp-based) in descending order
        backup_ids.sort_by(|a, b| b.cmp(a));

        debug!(
            "Found {} backups in environment {}",
            backup_ids.len(),
            environment
        );
        Ok(backup_ids)
    }

    async fn get_latest_environment_backup(&self, environment: &str) -> Result<Option<String>> {
        let backups = self.list_environment_backups(environment).await?;
        Ok(backups.into_iter().next())
    }

    async fn delete_backup(&self, backup_id: &str) -> Result<()> {
        // Try to parse the environment from the backup ID
        let environment = crate::database::backup_naming::get_environment_from_backup_id(backup_id)
            .unwrap_or_else(|| {
                debug!(
                    "Could not parse environment from backup ID {}, defaulting to 'dev'",
                    backup_id
                );
                String::from("dev")
            });

        let backup_path = self.get_backup_path(backup_id, &environment);

        if backup_path.exists() {
            info!("Deleting backup {} at {:?}", backup_id, backup_path);
            tokio_fs::remove_file(&backup_path).await.map_err(|e| {
                error!("Failed to delete backup file {:?}: {}", backup_path, e);
                DatabaseError::Io(e)
            })?;
            debug!("Successfully deleted backup {}", backup_id);
        } else {
            warn!(
                "Backup {} not found for deletion at {:?}",
                backup_id, backup_path
            );
        }

        Ok(())
    }

    async fn backup_exists(&self, backup_id: &str) -> Result<bool> {
        // Try to parse the environment from the backup ID
        let environment = crate::database::backup_naming::get_environment_from_backup_id(backup_id)
            .unwrap_or_else(|| String::from("dev")); // Default to dev if parsing fails

        let backup_path = self.get_backup_path(backup_id, &environment);
        Ok(backup_path.exists())
    }

    async fn read_backup(&self, backup_id: &str) -> Result<Vec<u8>> {
        // Try to parse the environment from the backup ID
        let environment = crate::database::backup_naming::get_environment_from_backup_id(backup_id)
            .unwrap_or_else(|| {
                debug!(
                    "Could not parse environment from backup ID {}, defaulting to 'dev'",
                    backup_id
                );
                String::from("dev")
            });

        let backup_path = self.get_backup_path(backup_id, &environment);
        info!(
            "Reading backup {} from local storage: {:?}",
            backup_id, backup_path
        );

        if !backup_path.exists() {
            warn!("Backup {} not found at {:?}", backup_id, backup_path);
            return Err(DatabaseError::BackupNotFound);
        }

        // Read the backup file contents
        let data = tokio_fs::read(&backup_path).await.map_err(|e| {
            error!("Failed to read backup file {:?}: {}", backup_path, e);
            DatabaseError::Io(e)
        })?;

        debug!(
            "Successfully read backup {} ({} bytes)",
            backup_id,
            data.len()
        );
        Ok(data)
    }

    async fn cleanup_old_backups(&self, keep_count: usize) -> Result<()> {
        info!(
            "Starting cleanup of old backups, keeping {} most recent",
            keep_count
        );
        let backups = self.list_backups().await?;

        // If we have more backups than the limit, delete the oldest ones
        if backups.len() > keep_count {
            let to_delete = backups.len() - keep_count;
            info!(
                "Found {} backups, deleting {} oldest",
                backups.len(),
                to_delete
            );

            for backup_id in backups.iter().skip(keep_count) {
                self.delete_backup(backup_id).await?;
            }

            info!("Cleanup completed, deleted {} backups", to_delete);
        } else {
            info!("No cleanup needed, only {} backups exist", backups.len());
        }

        Ok(())
    }

    async fn cleanup_environment_backups(
        &self,
        environment: &str,
        keep_count: usize,
    ) -> Result<()> {
        info!(
            "Starting cleanup of {} environment backups, keeping {} most recent",
            environment, keep_count
        );
        let backups = self.list_environment_backups(environment).await?;

        // If we have more backups than the limit, delete the oldest ones
        if backups.len() > keep_count {
            let to_delete = backups.len() - keep_count;
            info!(
                "Found {} backups in {}, deleting {} oldest",
                backups.len(),
                environment,
                to_delete
            );

            for backup_id in backups.iter().skip(keep_count) {
                self.delete_backup(backup_id).await?;
            }

            info!(
                "Cleanup completed for {}, deleted {} backups",
                environment, to_delete
            );
        } else {
            info!(
                "No cleanup needed for {}, only {} backups exist",
                environment,
                backups.len()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;

    // Helper function to create a test file with some content
    async fn create_test_file(path: &Path, content: &str) -> std::io::Result<()> {
        let mut file = tokio_fs::File::create(path).await?;
        file.write_all(content.as_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_store_and_retrieve_backup() {
        // Create a temporary directory for testing
        let temp_dir = tempdir().unwrap();
        let backup_dir = temp_dir.path().join("backups");

        // Create a test configuration
        let config = BackupConfig {
            local_backup_dir: backup_dir.clone(),
            local_backup_max_count: 5,
            ..Default::default()
        };

        let provider = LocalStorageProvider::new(&config);

        // Create a test backup file
        let source_file = temp_dir.path().join("test.db");
        create_test_file(&source_file, "test backup data")
            .await
            .unwrap();

        // Store the backup
        let backup_id = "test123";
        provider
            .store_backup(&source_file, backup_id, "dev")
            .await
            .unwrap();

        // Verify the backup exists
        assert!(provider.backup_exists(backup_id).await.unwrap());

        // Retrieve the backup to a new location
        let retrieved_file = temp_dir.path().join("retrieved.db");
        provider
            .retrieve_backup(backup_id, &retrieved_file)
            .await
            .unwrap();

        // Verify the retrieved file content
        let content = tokio_fs::read_to_string(&retrieved_file).await.unwrap();
        assert_eq!(content, "test backup data");
    }

    #[tokio::test]
    async fn test_list_and_cleanup_backups() {
        // Create a temporary directory for testing
        let temp_dir = tempdir().unwrap();
        let backup_dir = temp_dir.path().join("backups");

        // Create a test configuration
        let config = BackupConfig {
            local_backup_dir: backup_dir.clone(),
            local_backup_max_count: 2,
            ..Default::default()
        };

        let provider = LocalStorageProvider::new(&config);

        // Create a test backup file
        let source_file = temp_dir.path().join("test.db");
        create_test_file(&source_file, "test backup data")
            .await
            .unwrap();

        // Store multiple backups
        let backup_ids = ["001", "002", "003"];
        for id in &backup_ids {
            provider
                .store_backup(&source_file, id, "dev")
                .await
                .unwrap();
        }

        // List backups and verify count
        let backups = provider.list_backups().await.unwrap();
        assert_eq!(backups.len(), 3);

        // Get latest backup
        let latest = provider.get_latest_backup().await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap(), "003");

        // Clean up old backups (keep 2)
        provider.cleanup_old_backups(2).await.unwrap();

        // Verify only 2 backups remain
        let backups = provider.list_backups().await.unwrap();
        assert_eq!(backups.len(), 2);
        assert!(backups.contains(&"003".to_string()));
        assert!(backups.contains(&"002".to_string()));
        assert!(!backups.contains(&"001".to_string()));
    }
}
