//! Storage module for database backup/restore operations
//! 
//! This module provides an abstraction layer for storage operations,
//! allowing the application to seamlessly switch between AWS S3 and
//! local filesystem storage based on configuration and availability.

use std::path::Path;
use async_trait::async_trait;

use crate::config::BackupConfig;
use crate::database::Result;

// Re-export storage implementations
pub mod local_storage;
pub mod s3_storage;

/// Provides a unified interface for backup storage operations
/// 
/// This trait allows the application to abstract away the details of
/// where backups are stored (AWS S3 vs local filesystem) and provides
/// a consistent interface for backup and restore operations.
#[async_trait]
pub trait StorageProvider: Send + Sync {
    /// Store a backup file in the storage backend
    /// 
    /// # Arguments
    /// * `backup_path` - Path to the backup file to store
    /// * `backup_id` - Unique identifier for the backup (timestamp-based)
    /// * `environment` - Environment identifier (e.g., "dev", "prod")
    async fn store_backup(&self, backup_path: &Path, backup_id: &str, environment: &str) -> Result<()>;
    
    /// Retrieve a backup file from storage
    /// 
    /// # Arguments
    /// * `backup_id` - Identifier of the backup to retrieve
    /// * `destination_path` - Path where the backup should be saved locally
    async fn retrieve_backup(&self, backup_id: &str, destination_path: &Path) -> Result<()>;
    
    /// List all available backups in storage
    /// 
    /// Returns a list of backup IDs sorted by creation time (newest first)
    async fn list_backups(&self) -> Result<Vec<String>>;
    
    /// List all available backups for a specific environment
    /// 
    /// # Arguments
    /// * `environment` - Environment identifier (e.g., "dev", "prod")
    /// 
    /// Returns a list of backup IDs sorted by creation time (newest first)
    async fn list_environment_backups(&self, environment: &str) -> Result<Vec<String>>;
    
    /// Get the latest backup ID from storage
    /// 
    /// Returns the ID of the most recent backup, or None if no backups exist
    async fn get_latest_backup(&self) -> Result<Option<String>>;
    
    /// Get the latest backup ID for a specific environment
    /// 
    /// # Arguments
    /// * `environment` - Environment identifier (e.g., "dev", "prod")
    /// 
    /// Returns the ID of the most recent backup for the environment, or None if no backups exist
    async fn get_latest_environment_backup(&self, environment: &str) -> Result<Option<String>>;
    
    /// Delete a backup from storage
    /// 
    /// # Arguments
    /// * `backup_id` - Identifier of the backup to delete
    async fn delete_backup(&self, backup_id: &str) -> Result<()>;
    
    /// Check if a backup exists in storage
    /// 
    /// # Arguments
    /// * `backup_id` - Identifier of the backup to check
    async fn backup_exists(&self, backup_id: &str) -> Result<bool>;
    
    /// Clean up old backups, keeping only the most recent ones
    /// 
    /// # Arguments
    /// * `keep_count` - Number of most recent backups to keep
    async fn cleanup_old_backups(&self, keep_count: usize) -> Result<()>;
    
    /// Clean up old backups for a specific environment
    /// 
    /// # Arguments
    /// * `environment` - Environment identifier (e.g., "dev", "prod")
    /// * `keep_count` - Number of most recent backups to keep
    async fn cleanup_environment_backups(&self, environment: &str, keep_count: usize) -> Result<()>;
}

/// Create a storage provider based on the current configuration
/// 
/// This function will check AWS availability and create either an S3 storage
/// provider or fall back to local storage if AWS is unavailable.
pub async fn create_storage_provider(config: &BackupConfig) -> Result<Box<dyn StorageProvider>> {
    // Ensure the local backup directory exists (needed even if using AWS as fallback)
    config.ensure_local_backup_dir().map_err(|e| {
        crate::database::DatabaseError::Config(format!("Failed to create local backup directory: {}", e))
    })?;

    // Check if we should use AWS
    if config.should_use_aws().await {
        // Create and return S3 storage provider
        match s3_storage::S3StorageProvider::new(config).await {
            Ok(provider) => Ok(Box::new(provider)),
            Err(e) => {
                // Log the error and fall back to local storage
                eprintln!("Failed to create S3 storage provider: {}, falling back to local storage", e);
                Ok(Box::new(local_storage::LocalStorageProvider::new(config)))
            }
        }
    } else {
        // Use local storage
        Ok(Box::new(local_storage::LocalStorageProvider::new(config)))
    }
}