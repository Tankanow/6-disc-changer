//! Backup naming module for generating and parsing timestamp-based backup IDs
//!
//! This module provides functionality for creating structured, timestamp-based
//! backup identifiers that include environment information and are sortable
//! by creation time.

#[cfg(test)]
use chrono::TimeZone;
use chrono::{DateTime, Utc};
use rand::distributions::Alphanumeric;
use rand::{Rng, thread_rng};
use std::path::Path;

/// Service for generating backup identifiers
#[derive(Debug, Clone)]
pub struct BackupNamingService {
    /// Environment identifier (e.g., "dev", "prod")
    environment: String,
    /// Optional server identifier for multi-server deployments
    server_id: Option<String>,
}

impl BackupNamingService {
    /// Create a new naming service with the specified environment and optional server ID
    pub fn new(environment: &str, server_id: Option<&str>) -> Self {
        Self {
            environment: environment.to_string(),
            server_id: server_id.map(|s| s.to_string()),
        }
    }

    /// Generate a new backup ID using the current timestamp
    pub fn generate_backup_id(&self) -> String {
        self.generate_backup_id_with_time(Utc::now())
    }

    /// Generate a backup ID using the specified timestamp (primarily for testing)
    pub fn generate_backup_id_with_time(&self, timestamp: DateTime<Utc>) -> String {
        let date_part = timestamp.format("%Y-%m-%d");
        let time_part = timestamp.format("%H%M%S");

        // Generate a short random suffix
        let random_suffix = self.generate_random_suffix();

        let server_part = match &self.server_id {
            Some(id) => format!("_{}", id),
            None => String::new(),
        };

        format!(
            "backup_{}_{}_{}{}_{}",
            date_part, time_part, self.environment, server_part, random_suffix
        )
    }

    /// Generate a random alphanumeric suffix for uniqueness
    fn generate_random_suffix(&self) -> String {
        let mut rng = thread_rng();
        (0..6).map(|_| rng.sample(Alphanumeric) as char).collect()
    }

    /// Get the environment directory name for this naming service
    pub fn environment_dir(&self) -> String {
        self.environment.clone()
    }
}

/// Structured representation of a parsed backup ID
#[derive(Debug, Clone, PartialEq)]
pub struct BackupId {
    /// Original backup ID string
    id: String,
    /// Timestamp when the backup was created
    timestamp: DateTime<Utc>,
    /// Environment identifier (e.g., "dev", "prod")
    environment: String,
    /// Optional server identifier for multi-server deployments
    server_id: Option<String>,
    /// Random suffix for uniqueness
    random_suffix: String,
}

impl BackupId {
    /// Parse a backup ID string into a structured representation
    ///
    /// Format: backup_{DATE}_{TIME}_{ENV}_{SERVER_ID}_{RANDOM}
    /// or:     backup_{DATE}_{TIME}_{ENV}_{RANDOM}
    pub fn parse(backup_id: &str) -> Option<Self> {
        let parts: Vec<&str> = backup_id.split('_').collect();

        println!("Parsing {:?}", parts);
        if parts.len() < 5 || parts[0] != "backup" {
            return None;
        }

        // Parse date and time
        let date_str = parts[1];
        let time_str = parts[2];

        // Parse into hours, minutes, seconds
        let hours = &time_str[0..2];
        let minutes = &time_str[2..4];
        let seconds = &time_str[4..6];

        // Reformat for proper ISO parsing
        println!("Reformatting date and time");
        let iso_datetime = format!("{}T{}:{}:{}+00:00", date_str, hours, minutes, seconds);
        println!("Iso datetime: {}", iso_datetime);
        let timestamp = match DateTime::parse_from_rfc3339(&iso_datetime) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => return None,
        };

        println!("Timestamp is {}", timestamp);
        // Parse environment and optional server ID
        let environment = parts[3].to_string();

        let (server_id, random_suffix) = if parts.len() > 5 {
            // Has server ID
            (Some(parts[4].to_string()), parts[5].to_string())
        } else {
            // No server ID
            (None, parts[4].to_string())
        };

        Some(Self {
            id: backup_id.to_string(),
            timestamp,
            environment,
            server_id,
            random_suffix,
        })
    }

    /// Get the original backup ID string
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the timestamp when the backup was created
    pub fn timestamp(&self) -> &DateTime<Utc> {
        &self.timestamp
    }

    /// Get the environment identifier
    pub fn environment(&self) -> &str {
        &self.environment
    }

    /// Get the server identifier, if any
    pub fn server_id(&self) -> Option<&str> {
        self.server_id.as_deref()
    }

    /// Get the random suffix
    pub fn random_suffix(&self) -> &str {
        &self.random_suffix
    }
}

/// Get the environment directory from a backup ID string
pub fn get_environment_from_backup_id(backup_id: &str) -> Option<String> {
    BackupId::parse(backup_id).map(|id| id.environment().to_string())
}

/// Get the storage path for a backup with the given ID
pub fn get_backup_storage_path<P: AsRef<Path>>(
    base_dir: P,
    backup_id: &str,
) -> Option<std::path::PathBuf> {
    let env = get_environment_from_backup_id(backup_id)?;
    let path = base_dir
        .as_ref()
        .join(env)
        .join(format!("{}.db", backup_id));
    Some(path)
}

/// Get the S3 key for a backup with the given ID
pub fn get_backup_s3_key(prefix: &str, backup_id: &str) -> Option<String> {
    let env = get_environment_from_backup_id(backup_id)?;
    let key = format!("{}/{}/backup-{}.db", prefix, env, backup_id);
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_generate_backup_id() {
        let service = BackupNamingService::new("dev", None);

        // Create a fixed timestamp for testing
        let timestamp = Utc.with_ymd_and_hms(2025, 6, 1, 14, 30, 0).unwrap();

        let backup_id = service.generate_backup_id_with_time(timestamp);

        // The format should be backup_2025-06-01_143000_dev_RANDOM
        assert!(backup_id.starts_with("backup_2025-06-01_143000_dev_"));
        assert_eq!(backup_id.len(), 35); // Fixed length with 6-char random suffix
    }

    #[test]
    fn test_generate_backup_id_with_server() {
        let service = BackupNamingService::new("prod", Some("server1"));

        // Create a fixed timestamp for testing
        let timestamp = Utc.with_ymd_and_hms(2025, 6, 1, 14, 30, 0).unwrap();

        let backup_id = service.generate_backup_id_with_time(timestamp);

        // The format should be backup_2025-06-01_143000_prod_server1_RANDOM
        assert!(backup_id.starts_with("backup_2025-06-01_143000_prod_server1_"));
    }

    #[test]
    fn test_parse_backup_id() {
        // Test parsing a backup ID without server ID
        let backup_id = "backup_2025-06-01_143000_dev_abcdef";
        let parsed = BackupId::parse(backup_id).unwrap();

        assert_eq!(parsed.id(), backup_id);
        assert_eq!(parsed.environment(), "dev");
        assert_eq!(parsed.server_id(), None);
        assert_eq!(parsed.random_suffix(), "abcdef");

        let expected_timestamp = Utc.with_ymd_and_hms(2025, 6, 1, 14, 30, 0).unwrap();
        assert_eq!(*parsed.timestamp(), expected_timestamp);

        // Test parsing a backup ID with server ID
        let backup_id = "backup_2025-06-01_143000_prod_server1_abcdef";
        let parsed = BackupId::parse(backup_id).unwrap();

        assert_eq!(parsed.environment(), "prod");
        assert_eq!(parsed.server_id(), Some("server1"));
        assert_eq!(parsed.random_suffix(), "abcdef");
    }

    #[test]
    fn test_parse_invalid_backup_id() {
        // Too few parts
        assert!(BackupId::parse("backup_2025-06-01_143000_dev").is_none());

        // Wrong prefix
        assert!(BackupId::parse("wrong_2025-06-01_143000_dev_abcdef").is_none());

        // Invalid date format
        assert!(BackupId::parse("backup_20250601_143000_dev_abcdef").is_none());
    }

    #[test]
    fn test_get_environment_from_backup_id() {
        assert_eq!(
            get_environment_from_backup_id("backup_2025-06-01_143000_dev_abcdef"),
            Some("dev".to_string())
        );

        assert_eq!(
            get_environment_from_backup_id("backup_2025-06-01_143000_prod_server1_abcdef"),
            Some("prod".to_string())
        );

        assert_eq!(get_environment_from_backup_id("invalid"), None);
    }

    #[test]
    fn test_get_backup_storage_path() {
        let base_dir = Path::new("/backups");
        let backup_id = "backup_2025-06-01_143000_dev_abcdef";

        let path = get_backup_storage_path(base_dir, backup_id).unwrap();
        assert_eq!(
            path,
            Path::new("/backups/dev/backup_2025-06-01_143000_dev_abcdef.db")
        );
    }

    #[test]
    fn test_get_backup_s3_key() {
        let backup_id = "backup_2025-06-01_143000_dev_abcdef";

        let key = get_backup_s3_key("backups", backup_id).unwrap();
        assert_eq!(
            key,
            "backups/dev/backup-backup_2025-06-01_143000_dev_abcdef.db"
        );
    }
}
