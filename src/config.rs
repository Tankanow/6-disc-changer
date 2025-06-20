use std::env;
use std::path::PathBuf;
use tracing::error;

/// Configuration for database backup and restore functionality
#[derive(Debug, Clone)]
pub struct BackupConfig {
    /// Path to the SQLite database file
    pub database_path: PathBuf,
    /// Whether AWS S3 should be used for backup storage
    pub use_aws: bool,
    /// S3 bucket name for database backups
    pub s3_bucket_name: String,
    /// S3 prefix/folder path for organizing backups
    pub s3_prefix: String,
    /// AWS region for S3 operations
    pub aws_region: String,
    /// AWS role ARN to assume for S3 operations
    pub aws_role_arn: Option<String>,
    /// Local directory path for backups when AWS is unavailable
    pub local_backup_dir: PathBuf,
    /// Maximum number of local backups to keep
    pub local_backup_max_count: usize,
    /// Environment identifier for backups (e.g., "dev", "prod")
    pub environment: String,
    /// Optional server identifier for multi-server deployments
    pub server_id: Option<String>,
    /// Backup interval in seconds
    pub backup_interval_seconds: u64,
    /// Shutdown backup timeout in seconds
    pub shutdown_backup_timeout_seconds: u64,
    /// Force database restoration on startup even if database exists
    pub force_restoration: bool,
    /// Skip restoration if no backups are available
    pub skip_restoration_if_no_backups: bool,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            database_path: PathBuf::from("./data/storage.db"),
            use_aws: false,
            s3_bucket_name: String::new(),
            s3_prefix: String::from("backups"),
            aws_region: String::from("us-west-2"),
            aws_role_arn: None,
            local_backup_dir: PathBuf::from("./backups"),
            local_backup_max_count: 10,
            environment: String::from("dev"),
            server_id: None,
            backup_interval_seconds: 300, // 5 minutes
            shutdown_backup_timeout_seconds: 30,
            force_restoration: false,
            skip_restoration_if_no_backups: true,
        }
    }
}

impl BackupConfig {
    /// Load backup configuration from environment variables
    pub fn from_env() -> Self {
        let database_path = env::var("DATABASE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/storage.db"));

        let use_aws = env::var("BACKUP_USE_AWS")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let s3_bucket_name = env::var("BACKUP_S3_BUCKET").unwrap_or_else(|_| String::new());

        let s3_prefix = env::var("BACKUP_S3_PREFIX").unwrap_or_else(|_| String::from("backups"));

        let aws_region = env::var("AWS_REGION").unwrap_or_else(|_| String::from("us-west-2"));

        let aws_role_arn = env::var("AWS_ROLE_ARN").ok();

        let local_backup_dir = env::var("BACKUP_LOCAL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./backups"));

        let local_backup_max_count = env::var("BACKUP_LOCAL_MAX_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let environment = env::var("BACKUP_ENVIRONMENT").unwrap_or_else(|_| String::from("dev"));

        let server_id = env::var("BACKUP_SERVER_ID").ok();

        let backup_interval_seconds = env::var("BACKUP_INTERVAL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        let shutdown_backup_timeout_seconds = env::var("BACKUP_SHUTDOWN_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let force_restoration = env::var("BACKUP_FORCE_RESTORATION")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let skip_restoration_if_no_backups = env::var("BACKUP_SKIP_RESTORATION_IF_NO_BACKUPS")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true);

        Self {
            database_path,
            use_aws,
            s3_bucket_name,
            s3_prefix,
            aws_region,
            aws_role_arn,
            local_backup_dir,
            local_backup_max_count,
            environment,
            server_id,
            backup_interval_seconds,
            shutdown_backup_timeout_seconds,
            force_restoration,
            skip_restoration_if_no_backups,
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
            client
                .head_bucket()
                .bucket(&self.s3_bucket_name)
                .send()
                .await
        }
        .await
        {
            Ok(_) => {
                // Bucket exists and is accessible
                true
            }
            Err(err) => {
                // Log the error and return false
                error!(
                    "AWS S3 connectivity check failed: {}, falling back to local storage",
                    err
                );
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

/// Main application configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Backup configuration
    pub backup: BackupConfig,
    /// Port for the web server
    pub port: u16,
    /// Host for the web server
    pub host: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backup: BackupConfig::default(),
            port: 8080,
            host: String::from("0.0.0.0"),
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let backup = BackupConfig::from_env();

        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        let host = env::var("HOST").unwrap_or_else(|_| String::from("0.0.0.0"));

        Self { backup, port, host }
    }
}
