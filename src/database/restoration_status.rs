use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tracks the status and statistics of database restoration operations
#[derive(Debug, Clone)]
pub struct RestorationStatus {
    /// Whether a restoration is currently in progress
    pub is_restoring: bool,
    /// Timestamp when the current restoration started (if any)
    pub current_restoration_started: Option<DateTime<Utc>>,
    /// Result of the last restoration attempt
    pub last_restoration_result: Option<RestorationResult>,
    /// Total number of successful restorations
    pub successful_restorations: u64,
    /// Total number of failed restorations
    pub failed_restorations: u64,
    /// Total bytes restored across all operations
    pub total_bytes_restored: u64,
    /// Average restoration duration
    pub average_restoration_duration: Option<Duration>,
}

/// Result of a restoration operation
#[derive(Debug, Clone)]
pub struct RestorationResult {
    /// When the restoration completed
    pub completed_at: DateTime<Utc>,
    /// Whether the restoration was successful
    pub success: bool,
    /// Error message if restoration failed
    pub error: Option<String>,
    /// Size of the restored database in bytes
    pub restored_size: Option<u64>,
    /// Duration of the restoration operation
    pub duration: Duration,
    /// Source of the restoration (e.g., S3 key or local path)
    pub source: String,
}

impl Default for RestorationStatus {
    fn default() -> Self {
        Self {
            is_restoring: false,
            current_restoration_started: None,
            last_restoration_result: None,
            successful_restorations: 0,
            failed_restorations: 0,
            total_bytes_restored: 0,
            average_restoration_duration: None,
        }
    }
}

impl RestorationStatus {
    /// Create a new RestorationStatus instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the start of a restoration operation
    pub fn start_restoration(&mut self) {
        self.is_restoring = true;
        self.current_restoration_started = Some(Utc::now());
    }

    /// Mark the completion of a restoration operation
    pub fn complete_restoration(
        &mut self,
        success: bool,
        error: Option<String>,
        restored_size: Option<u64>,
        source: String,
    ) {
        let started = self.current_restoration_started.unwrap_or_else(Utc::now);
        let duration =
            Duration::from_secs((Utc::now().timestamp() - started.timestamp()).max(0) as u64);

        self.is_restoring = false;
        self.current_restoration_started = None;

        if success {
            self.successful_restorations += 1;
            if let Some(size) = restored_size {
                self.total_bytes_restored += size;
            }
        } else {
            self.failed_restorations += 1;
        }

        // Update average duration
        let total_restorations = self.successful_restorations + self.failed_restorations;
        if total_restorations > 0 {
            let total_duration_secs = self
                .average_restoration_duration
                .map(|d| d.as_secs() * (total_restorations - 1))
                .unwrap_or(0)
                + duration.as_secs();
            self.average_restoration_duration = Some(Duration::from_secs(
                total_duration_secs / total_restorations,
            ));
        }

        self.last_restoration_result = Some(RestorationResult {
            completed_at: Utc::now(),
            success,
            error,
            restored_size,
            duration,
            source,
        });
    }

    /// Get the current restoration status as a formatted string
    pub fn status_string(&self) -> String {
        if self.is_restoring {
            if let Some(started) = self.current_restoration_started {
                let elapsed = Utc::now().timestamp() - started.timestamp();
                format!("Restoration in progress ({}s elapsed)", elapsed)
            } else {
                "Restoration in progress".to_string()
            }
        } else if let Some(result) = &self.last_restoration_result {
            if result.success {
                format!(
                    "Last restoration completed successfully at {} from {} ({} bytes in {:?})",
                    result.completed_at.format("%Y-%m-%d %H:%M:%S UTC"),
                    result.source,
                    result.restored_size.unwrap_or(0),
                    result.duration
                )
            } else {
                format!(
                    "Last restoration failed at {}: {}",
                    result.completed_at.format("%Y-%m-%d %H:%M:%S UTC"),
                    result.error.as_deref().unwrap_or("Unknown error")
                )
            }
        } else {
            "No restoration performed yet".to_string()
        }
    }
}

/// Thread-safe wrapper for RestorationStatus
pub type SharedRestorationStatus = Arc<Mutex<RestorationStatus>>;

/// Create a new shared restoration status instance
pub fn create_shared_restoration_status() -> SharedRestorationStatus {
    Arc::new(Mutex::new(RestorationStatus::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_restoration_status_creation() {
        let status = RestorationStatus::new();
        assert!(!status.is_restoring);
        assert!(status.current_restoration_started.is_none());
        assert!(status.last_restoration_result.is_none());
        assert_eq!(status.successful_restorations, 0);
        assert_eq!(status.failed_restorations, 0);
    }

    #[test]
    fn test_start_restoration() {
        let mut status = RestorationStatus::new();
        status.start_restoration();
        assert!(status.is_restoring);
        assert!(status.current_restoration_started.is_some());
    }

    #[test]
    fn test_complete_restoration_success() {
        let mut status = RestorationStatus::new();
        status.start_restoration();

        // Simulate some work
        thread::sleep(Duration::from_millis(10));

        status.complete_restoration(true, None, Some(1024), "s3://bucket/backup.db".to_string());

        assert!(!status.is_restoring);
        assert!(status.current_restoration_started.is_none());
        assert_eq!(status.successful_restorations, 1);
        assert_eq!(status.failed_restorations, 0);
        assert_eq!(status.total_bytes_restored, 1024);

        let result = status.last_restoration_result.as_ref().unwrap();
        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.restored_size, Some(1024));
        assert_eq!(result.source, "s3://bucket/backup.db");
    }

    #[test]
    fn test_complete_restoration_failure() {
        let mut status = RestorationStatus::new();
        status.start_restoration();

        status.complete_restoration(
            false,
            Some("Network error".to_string()),
            None,
            "s3://bucket/backup.db".to_string(),
        );

        assert!(!status.is_restoring);
        assert_eq!(status.successful_restorations, 0);
        assert_eq!(status.failed_restorations, 1);
        assert_eq!(status.total_bytes_restored, 0);

        let result = status.last_restoration_result.as_ref().unwrap();
        assert!(!result.success);
        assert_eq!(result.error, Some("Network error".to_string()));
    }

    #[test]
    fn test_status_string() {
        let mut status = RestorationStatus::new();

        // Initial state
        assert_eq!(status.status_string(), "No restoration performed yet");

        // During restoration
        status.start_restoration();
        assert!(
            status
                .status_string()
                .starts_with("Restoration in progress")
        );

        // After successful restoration
        status.complete_restoration(true, None, Some(2048), "local/backup.db".to_string());
        assert!(status.status_string().contains("completed successfully"));

        // After failed restoration
        status.complete_restoration(
            false,
            Some("Database corrupted".to_string()),
            None,
            "s3://bucket/backup.db".to_string(),
        );
        assert!(status.status_string().contains("failed"));
        assert!(status.status_string().contains("Database corrupted"));
    }

    #[test]
    fn test_shared_restoration_status() {
        let shared_status = create_shared_restoration_status();

        // Test concurrent access
        let status_clone = Arc::clone(&shared_status);
        let handle = thread::spawn(move || {
            let mut status = status_clone.lock().unwrap();
            status.start_restoration();
        });

        handle.join().unwrap();

        let status = shared_status.lock().unwrap();
        assert!(status.is_restoring);
    }
}
