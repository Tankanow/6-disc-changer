use std::env;
use std::path::PathBuf;

/// Configuration for database backup and restore functionality
#[derive(Debug, Clone)]
pub struct BackupConfig {
    /// Whether AWS S3 should be used for backup storage
    pub use_aws: bool,
    /// S3 bucket name for database backups
    pub s3_bucket_name: String,
    /// AWS region for S3 operations
    pub aws_region: String,
    /// AWS role ARN to assume for S3 operations
    pub aws_role_arn: Option<String>,
    /// Local directory path for backups when AWS is unavailable
    pub local_backup_dir: PathBuf,
    /// Maximum number of local backups to keep
    pub local_backup_max_count: usize,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            use_aws: false,
            s3_bucket_name: String::new(),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: PathBuf::from("./backups"),
            local_backup_max_count: 10,
        }
    }
}

impl BackupConfig {
    /// Load backup configuration from environment variables
    pub fn from_env() -> Self {
        let use_aws = env::var("BACKUP_USE_AWS")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let s3_bucket_name = env::var("BACKUP_S3_BUCKET")
            .unwrap_or_else(|_| String::new());

        let aws_region = env::var("AWS_REGION")
            .unwrap_or_else(|_| String::from("us-west-2"));

        let aws_role_arn = env::var("AWS_ROLE_ARN").ok();

        let local_backup_dir = env::var("BACKUP_LOCAL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./backups"));

        let local_backup_max_count = env::var("BACKUP_LOCAL_MAX_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        Self {
            use_aws,
            s3_bucket_name,
            aws_region,
            aws_role_arn,
            local_backup_dir,
            local_backup_max_count,
        }
    }

    /// Check if AWS should be used based on configuration and connectivity
    pub async fn should_use_aws(&self) -> bool {
        use aws_sdk_s3::Client as S3Client;
        use aws_types::region::Region;
        
        // If AWS is disabled in config, don't use it
        if !self.use_aws {
            return false;
        }

        // If bucket name is empty, can't use AWS
        if self.s3_bucket_name.is_empty() {
            return false;
        }

        // Try to initialize AWS client and check connectivity
        match async {
            // Configure AWS SDK
            let aws_config = aws_config::from_env()
                .region(Region::new(self.aws_region.clone()))
                .load()
                .await;

            // Create S3 client
            let client = S3Client::new(&aws_config);

            // Try to check if the bucket exists
            client.head_bucket()
                .bucket(&self.s3_bucket_name)
                .send()
                .await
        }.await {
            Ok(_) => {
                // Bucket exists and is accessible
                true
            }
            Err(err) => {
                // Log the error and return false
                eprintln!("AWS S3 connectivity check failed: {}, falling back to local storage", err);
                false
            }
        }
    }

    /// Ensure local backup directory exists
    pub fn ensure_local_backup_dir(&self) -> std::io::Result<()> {
        if !self.local_backup_dir.exists() {
            std::fs::create_dir_all(&self.local_backup_dir)?;
        }
        Ok(())
    }
}