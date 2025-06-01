//! AWS S3 implementation of the StorageProvider trait
//! 
//! This module provides an AWS S3 implementation of the StorageProvider
//! trait for storing database backups in Amazon S3.

use std::path::{Path, PathBuf};
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::error::SdkError;
use async_trait::async_trait;
use tokio::fs as tokio_fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info};

use crate::config::BackupConfig;
use crate::database::Result;
use crate::database::DatabaseError;
use super::StorageProvider;

/// Provides AWS S3 storage for database backups
pub struct S3StorageProvider {
    /// S3 client
    client: S3Client,
    /// S3 bucket name
    bucket: String,
    /// Prefix for backup objects
    prefix: String,
    /// Local temporary directory for downloads
    temp_dir: PathBuf,
}

impl S3StorageProvider {
    /// Create a new S3StorageProvider with the given configuration
    pub async fn new(config: &BackupConfig) -> Result<Self> {
        // Configure AWS SDK with default credential provider chain
        // This will automatically handle OIDC token exchange when AWS_ROLE_ARN is set
        let aws_config = aws_config::from_env()
            .region(aws_types::region::Region::new(config.aws_region.clone()))
            .load()
            .await;

        // Create S3 client
        let client = S3Client::new(&aws_config);

        // Verify that the bucket exists and is accessible
        match client.head_bucket().bucket(&config.s3_bucket_name).send().await {
            Ok(_) => {
                info!("Successfully connected to S3 bucket: {}", &config.s3_bucket_name);
            }
            Err(err) => {
                error!("Failed to access S3 bucket: {}: {}", &config.s3_bucket_name, err);
                return Err(DatabaseError::Storage(format!(
                    "Failed to access S3 bucket {}: {}", 
                    &config.s3_bucket_name, err
                )));
            }
        }

        Ok(Self {
            client,
            bucket: config.s3_bucket_name.clone(),
            prefix: "backups/".to_string(),
            temp_dir: config.local_backup_dir.clone(),
        })
    }

    /// Get the S3 key for a backup with the given ID
    fn get_backup_key(&self, backup_id: &str) -> String {
        format!("{}backup-{}.db", self.prefix, backup_id)
    }

    /// Extract backup ID from an S3 key
    fn extract_backup_id(&self, key: &str) -> Option<String> {
        // Extract backup ID from key (format: backups/backup-{id}.db)
        let prefix = format!("{}backup-", self.prefix);
        if key.starts_with(&prefix) && key.ends_with(".db") {
            let id = key
                .strip_prefix(&prefix)
                .unwrap()
                .strip_suffix(".db")
                .unwrap();
            Some(id.to_string())
        } else {
            None
        }
    }

    /// Map AWS S3 errors to DatabaseError
    fn map_s3_error<E: std::fmt::Debug>(&self, error: SdkError<E>, operation: &str) -> DatabaseError {
        match &error {
            SdkError::ConstructionFailure(_) => {
                DatabaseError::Storage(format!("S3 client construction error during {}: {:?}", operation, error))
            }
            SdkError::DispatchFailure(err) => {
                DatabaseError::Storage(format!("S3 dispatch error during {}: {:?}", operation, err))
            }
            SdkError::ResponseError(err) => {
                DatabaseError::Storage(format!("S3 response error during {}: {:?}", operation, err))
            }
            SdkError::TimeoutError(_) => {
                DatabaseError::Storage(format!("S3 timeout during {}: {:?}", operation, error))
            }
            SdkError::ServiceError(service_err) => {
                // For the service error, we check for 404 (Not Found) errors
                let status_code = service_err.raw().http().status();
                match service_err.err() {
                    // Handle specific error types based on the operation
                    _ => {
                        if operation == "retrieve_backup" && status_code == 404 {
                            DatabaseError::BackupNotFound
                        } else {
                            DatabaseError::Storage(format!("S3 service error during {}: {:?}", operation, error))
                        }
                    }
                }
            }
            _ => DatabaseError::Storage(format!("Unknown S3 error during {}: {:?}", operation, error)),
        }
    }
}

#[async_trait]
impl StorageProvider for S3StorageProvider {
    async fn store_backup(&self, backup_path: &Path, backup_id: &str) -> Result<()> {
        // Read the backup file
        let body = match tokio_fs::read(backup_path).await {
            Ok(content) => content,
            Err(e) => return Err(DatabaseError::Io(e)),
        };

        let key = self.get_backup_key(backup_id);
        
        // Upload to S3
        debug!("Uploading backup {} to S3 bucket {} with key {}", backup_id, self.bucket, key);
        match self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(body.into())
            .send()
            .await
        {
            Ok(_) => {
                info!("Successfully uploaded backup {} to S3", backup_id);
                Ok(())
            }
            Err(err) => {
                error!("Failed to upload backup to S3: {}", err);
                Err(self.map_s3_error(err, "store_backup"))
            }
        }
    }
    
    async fn retrieve_backup(&self, backup_id: &str, destination_path: &Path) -> Result<()> {
        let key = self.get_backup_key(backup_id);
        
        debug!("Retrieving backup {} from S3 bucket {} with key {}", backup_id, self.bucket, key);
        
        // Download from S3
        let resp = match self.client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                error!("Failed to retrieve backup from S3: {}", err);
                return Err(self.map_s3_error(err, "retrieve_backup"));
            }
        };
        
        // Create the parent directory if it doesn't exist
        if let Some(parent) = destination_path.parent() {
            if !parent.exists() {
                tokio_fs::create_dir_all(parent).await
                    .map_err(|e| DatabaseError::Io(e))?;
            }
        }
        
        // Write the backup data to the destination file
        let body = resp.body.collect().await
            .map_err(|e| DatabaseError::Storage(format!("Failed to read S3 response body: {}", e)))?;
        
        let bytes = body.into_bytes();
        
        let mut file = tokio_fs::File::create(destination_path).await
            .map_err(|e| DatabaseError::Io(e))?;
        
        file.write_all(&bytes).await
            .map_err(|e| DatabaseError::Io(e))?;
        
        file.flush().await
            .map_err(|e| DatabaseError::Io(e))?;
        
        info!("Successfully retrieved backup {} from S3", backup_id);
        Ok(())
    }
    
    async fn list_backups(&self) -> Result<Vec<String>> {
        debug!("Listing backups in S3 bucket {}", self.bucket);
        
        // List objects in the bucket with the backup prefix
        let resp = match self.client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&self.prefix)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                error!("Failed to list backups in S3: {}", err);
                return Err(self.map_s3_error(err, "list_backups"));
            }
        };
        
        // Extract backup IDs from the object keys
        let mut backup_ids = Vec::new();
        
        if let Some(objects) = resp.contents {
            for obj in objects {
                if let Some(key) = &obj.key {
                    if let Some(id) = self.extract_backup_id(key) {
                        backup_ids.push(id);
                    }
                }
            }
        }
        
        // Sort backups by ID (which is timestamp) in descending order
        backup_ids.sort_by(|a, b| b.cmp(a));
        
        debug!("Found {} backups in S3", backup_ids.len());
        Ok(backup_ids)
    }
    
    async fn get_latest_backup(&self) -> Result<Option<String>> {
        let backups = self.list_backups().await?;
        Ok(backups.into_iter().next())
    }
    
    async fn delete_backup(&self, backup_id: &str) -> Result<()> {
        let key = self.get_backup_key(backup_id);
        
        debug!("Deleting backup {} from S3 bucket {} with key {}", backup_id, self.bucket, key);
        
        // Delete the object from S3
        match self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => {
                info!("Successfully deleted backup {} from S3", backup_id);
                Ok(())
            }
            Err(err) => {
                error!("Failed to delete backup from S3: {}", err);
                Err(self.map_s3_error(err, "delete_backup"))
            }
        }
    }
    
    async fn backup_exists(&self, backup_id: &str) -> Result<bool> {
        let key = self.get_backup_key(backup_id);
        
        debug!("Checking if backup {} exists in S3 bucket {} with key {}", backup_id, self.bucket, key);
        
        // Check if the object exists in S3
        match self.client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => {
                debug!("Backup {} exists in S3", backup_id);
                Ok(true)
            }
            Err(err) => {
                // If the error is a 404, the backup doesn't exist
                match &err {
                    SdkError::ServiceError(service_err) => {
                        let status_code = service_err.raw().http().status();
                        if status_code == 404 {
                            debug!("Backup {} does not exist in S3", backup_id);
                            return Ok(false);
                        }
                    }
                    _ => {}
                }
                
                // For any other error, return an error
                error!("Failed to check if backup exists in S3: {}", err);
                Err(self.map_s3_error(err, "backup_exists"))
            }
        }
    }
    
    async fn cleanup_old_backups(&self, keep_count: usize) -> Result<()> {
        let backups = self.list_backups().await?;
        
        // If we have more backups than the limit, delete the oldest ones
        if backups.len() > keep_count {
            info!("Cleaning up old backups in S3, keeping {} most recent", keep_count);
            
            for backup_id in backups.iter().skip(keep_count) {
                debug!("Deleting old backup {}", backup_id);
                self.delete_backup(backup_id).await?;
            }
            
            info!("Successfully cleaned up old backups in S3");
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::mock;
    use std::path::PathBuf;

    // We'll use mockall to mock the AWS S3 client for testing
    mock! {
        S3Client {}
        
        #[async_trait]
        impl Clone for S3Client {
            fn clone(&self) -> Self;
        }
        
        // Add mock methods for the S3 client methods we use
    }

    // This test would use a real S3 bucket or a mocked S3 client
    // We'll skip the actual implementation for now
    #[tokio::test]
    #[ignore]
    async fn test_s3_operations() {
        // This would be a full integration test with AWS S3
        // For now, we'll just ensure the code compiles
    }
    
    // Unit tests for helper methods
    #[test]
    fn test_get_backup_key() {
        // Create a provider with dummy client for testing
        let config = aws_types::SdkConfig::builder().build();
        let client = S3Client::new(&config);
        
        let provider = S3StorageProvider {
            client,
            bucket: "test-bucket".to_string(),
            prefix: "backups/".to_string(),
            temp_dir: PathBuf::from("/tmp"),
        };
        
        assert_eq!(provider.get_backup_key("123"), "backups/backup-123.db");
    }
    
    #[test]
    fn test_extract_backup_id() {
        // Create a provider with dummy client for testing
        let config = aws_types::SdkConfig::builder().build();
        let client = S3Client::new(&config);
        
        let provider = S3StorageProvider {
            client,
            bucket: "test-bucket".to_string(),
            prefix: "backups/".to_string(),
            temp_dir: PathBuf::from("/tmp"),
        };
        
        assert_eq!(provider.extract_backup_id("backups/backup-123.db"), Some("123".to_string()));
        assert_eq!(provider.extract_backup_id("backups/something-else.db"), None);
    }
}