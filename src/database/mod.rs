//! Database module for SQLite database operations including backup and restore functionality

// Re-export storage module for public use
pub mod storage;

// Database modules
pub mod backup;
pub mod backup_naming;
pub mod backup_status;
pub mod restoration_status;
pub mod restore;
pub mod scheduler;
// mod s3_client;

// Public re-exports
pub use backup::BackupManager;
pub use backup_status::create_shared_status;
pub use restoration_status::{
    RestorationStatus, SharedRestorationStatus, create_shared_restoration_status,
};
pub use restore::RestorationChecker;
pub use scheduler::BackupScheduler;

/// SQLite database file path
pub const DATABASE_PATH: &str = "db.sqlite";

/// Directory for database-related files like backups
pub const DATABASE_DIR: &str = "database";

/// Database error type for backup/restore operations
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("SQLite error: {0}")]
    Sqlite(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Backup not found")]
    BackupNotFound,

    #[error("AWS error: {0}")]
    Aws(String),

    #[error("Backup error: {0}")]
    Backup(String),

    #[error("Backup already in progress")]
    BackupInProgress,

    #[error("Restoration error: {0}")]
    Restoration(String),

    #[error("File system error: {0}")]
    FileSystem(String),
}

/// Result type for database operations
pub type Result<T> = std::result::Result<T, DatabaseError>;
pub type DatabaseResult<T> = Result<T>;
