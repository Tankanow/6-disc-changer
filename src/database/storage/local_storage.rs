//! Local filesystem implementation of the StorageProvider trait
//! 
//! This module provides a local filesystem implementation of the StorageProvider
//! trait for development and fallback when AWS S3 is unavailable.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use async_trait::async_trait;
use tokio::fs as tokio_fs;
use crate::config::BackupConfig;
use crate::database::Result;
use crate::database::DatabaseError;

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
    async fn store_backup(&self, backup_path: &Path, backup_id: &str, environment: &str) -> Result<()> {
        // Ensure the environment backup directory exists
        let env_dir = self.backup_dir.join(environment);
        if !env_dir.exists() {
            tokio_fs::create_dir_all(&env_dir).await
                .map_err(|e| DatabaseError::Io(e))?;
        }

        let dest_path = self.get_backup_path(backup_id, environment);
        
        // Copy the backup file to the backup directory
        tokio_fs::copy(backup_path, &dest_path).await
            .map_err(|e| DatabaseError::Io(e))?;
            
        Ok(())
    }
    
    async fn retrieve_backup(&self, backup_id: &str, destination_path: &Path) -> Result<()> {
        // Try to parse the environment from the backup ID
        let environment = crate::database::backup_naming::get_environment_from_backup_id(backup_id)
            .unwrap_or_else(|| String::from("dev")); // Default to dev if parsing fails
            
        let source_path = self.get_backup_path(backup_id, &environment);
        
        if !source_path.exists() {
            return Err(DatabaseError::BackupNotFound);
        }
        
        // Create the parent directory if it doesn't exist
        if let Some(parent) = destination_path.parent() {
            if !parent.exists() {
                tokio_fs::create_dir_all(parent).await
                    .map_err(|e| DatabaseError::Io(e))?;
            }
        }
        
        // Copy the backup file to the destination
        tokio_fs::copy(&source_path, destination_path).await
            .map_err(|e| DatabaseError::Io(e))?;
            
        Ok(())
    }
    
    async fn list_backups(&self) -> Result<Vec<String>> {
        if !self.backup_dir.exists() {
            return Ok(Vec::new());
        }
        
        let mut all_backups = Vec::new();
        
        // Read all environment directories
        let mut dir_entries = tokio_fs::read_dir(&self.backup_dir).await
            .map_err(|e| DatabaseError::Io(e))?;
            
        // Iterate through environment directories
        while let Some(env_entry) = dir_entries.next_entry().await.map_err(|e| DatabaseError::Io(e))? {
            let env_path = env_entry.path();
            
            if env_path.is_dir() {
                // List backups in this environment
                let env_backups = self.list_environment_backups(
                    env_path.file_name().unwrap().to_str().unwrap()
                ).await?;
                
                all_backups.extend(env_backups);
            }
        }
        
        // Sort all backups by ID (which is timestamp-based) in descending order
        all_backups.sort_by(|a, b| b.cmp(a));
        
        Ok(all_backups)
    }
    
    async fn get_latest_backup(&self) -> Result<Option<String>> {
        let backups = self.list_backups().await?;
        Ok(backups.into_iter().next())
    }
    
    async fn list_environment_backups(&self, environment: &str) -> Result<Vec<String>> {
        let env_dir = self.backup_dir.join(environment);
        
        if !env_dir.exists() {
            return Ok(Vec::new());
        }
        
        let mut entries = tokio_fs::read_dir(&env_dir).await
            .map_err(|e| DatabaseError::Io(e))?;
            
        let mut backup_ids = Vec::new();
        
        while let Some(entry) = entries.next_entry().await.map_err(|e| DatabaseError::Io(e))? {
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
        
        Ok(backup_ids)
    }
    
    async fn get_latest_environment_backup(&self, environment: &str) -> Result<Option<String>> {
        let backups = self.list_environment_backups(environment).await?;
        Ok(backups.into_iter().next())
    }
    
    async fn delete_backup(&self, backup_id: &str) -> Result<()> {
        // Try to parse the environment from the backup ID
        let environment = crate::database::backup_naming::get_environment_from_backup_id(backup_id)
            .unwrap_or_else(|| String::from("dev")); // Default to dev if parsing fails
            
        let backup_path = self.get_backup_path(backup_id, &environment);
        
        if backup_path.exists() {
            tokio_fs::remove_file(backup_path).await
                .map_err(|e| DatabaseError::Io(e))?;
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
    
    async fn cleanup_old_backups(&self, keep_count: usize) -> Result<()> {
        let backups = self.list_backups().await?;
        
        // If we have more backups than the limit, delete the oldest ones
        if backups.len() > keep_count {
            for backup_id in backups.iter().skip(keep_count) {
                self.delete_backup(backup_id).await?;
            }
        }
        
        Ok(())
    }
    
    async fn cleanup_environment_backups(&self, environment: &str, keep_count: usize) -> Result<()> {
        let backups = self.list_environment_backups(environment).await?;
        
        // If we have more backups than the limit, delete the oldest ones
        if backups.len() > keep_count {
            for backup_id in backups.iter().skip(keep_count) {
                self.delete_backup(backup_id).await?;
            }
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
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: backup_dir.clone(),
            local_backup_max_count: 5,
        };
        
        let provider = LocalStorageProvider::new(&config);
        
        // Create a test backup file
        let source_file = temp_dir.path().join("test.db");
        create_test_file(&source_file, "test backup data").await.unwrap();
        
        // Store the backup
        let backup_id = "test123";
        provider.store_backup(&source_file, backup_id, "dev").await.unwrap();
        
        // Verify the backup exists
        assert!(provider.backup_exists(backup_id).await.unwrap());
        
        // Retrieve the backup to a new location
        let retrieved_file = temp_dir.path().join("retrieved.db");
        provider.retrieve_backup(backup_id, &retrieved_file).await.unwrap();
        
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
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: backup_dir.clone(),
            local_backup_max_count: 2,
        };
        
        let provider = LocalStorageProvider::new(&config);
        
        // Create a test backup file
        let source_file = temp_dir.path().join("test.db");
        create_test_file(&source_file, "test backup data").await.unwrap();
        
        // Store multiple backups
        let backup_ids = ["001", "002", "003"];
        for id in &backup_ids {
            provider.store_backup(&source_file, id, "dev").await.unwrap();
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