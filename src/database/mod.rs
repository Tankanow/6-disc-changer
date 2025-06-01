//! Database module for SQLite database operations including backup and restore functionality

// Re-export storage module for public use
pub mod storage;

// Database modules
pub mod backup;
// These will be implemented in future tasks
// mod restore;
// mod scheduler;
// mod s3_client;

// Public re-exports
// pub use backup::*;  // Will be uncommented when used by other modules
// pub use restore::*;
// pub use scheduler::*;

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
}

/// Result type for database operations
pub type Result<T> = std::result::Result<T, DatabaseError>;