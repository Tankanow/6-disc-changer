use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Maximum number of backup results to keep in history
const MAX_HISTORY_SIZE: usize = 100;

/// Represents the current state of a backup operation
#[derive(Debug, Clone, PartialEq)]
pub enum BackupState {
    /// No backup is currently running
    Idle,
    /// A backup is currently in progress
    InProgress { started_at: DateTime<Utc> },
    /// The last backup completed successfully
    Completed {
        completed_at: DateTime<Utc>,
        duration: Duration,
        size_bytes: u64,
    },
    /// The last backup failed
    Failed {
        failed_at: DateTime<Utc>,
        duration: Duration,
        error: String,
    },
}

/// Records the result of a single backup operation
#[derive(Debug, Clone)]
pub struct BackupResult {
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration: Duration,
    pub success: bool,
    pub size_bytes: Option<u64>,
    pub error: Option<String>,
}

/// Tracks the status and history of backup operations
#[derive(Debug)]
pub struct BackupStatus {
    /// Current state of the backup system
    pub state: BackupState,
    /// Timestamp of the last successful backup
    pub last_successful_backup: Option<DateTime<Utc>>,
    /// Total number of successful backups
    pub success_count: u64,
    /// Total number of failed backups
    pub failure_count: u64,
    /// History of recent backup results
    pub history: VecDeque<BackupResult>,
}

impl Default for BackupStatus {
    fn default() -> Self {
        Self {
            state: BackupState::Idle,
            last_successful_backup: None,
            success_count: 0,
            failure_count: 0,
            history: VecDeque::with_capacity(MAX_HISTORY_SIZE),
        }
    }
}

impl BackupStatus {
    /// Creates a new BackupStatus instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempts to start a new backup, returns false if one is already in progress
    pub fn start_backup(&mut self) -> bool {
        match &self.state {
            BackupState::InProgress { .. } => false,
            _ => {
                self.state = BackupState::InProgress {
                    started_at: Utc::now(),
                };
                true
            }
        }
    }

    /// Records a successful backup completion
    pub fn complete_backup(&mut self, size_bytes: u64) {
        if let BackupState::InProgress { started_at } = self.state {
            let completed_at = Utc::now();
            let duration = (completed_at - started_at)
                .to_std()
                .unwrap_or(Duration::from_secs(0));

            // Add to history
            let result = BackupResult {
                started_at,
                completed_at,
                duration,
                success: true,
                size_bytes: Some(size_bytes),
                error: None,
            };
            self.add_to_history(result);

            self.state = BackupState::Completed {
                completed_at,
                duration,
                size_bytes,
            };

            self.last_successful_backup = Some(completed_at);
            self.success_count += 1;
        }
    }

    /// Records a failed backup
    pub fn fail_backup(&mut self, error: String) {
        if let BackupState::InProgress { started_at } = self.state {
            let failed_at = Utc::now();
            let duration = (failed_at - started_at)
                .to_std()
                .unwrap_or(Duration::from_secs(0));

            // Add to history
            let result = BackupResult {
                started_at,
                completed_at: failed_at,
                duration,
                success: false,
                size_bytes: None,
                error: Some(error.clone()),
            };
            self.add_to_history(result);

            self.state = BackupState::Failed {
                failed_at,
                duration,
                error,
            };

            self.failure_count += 1;
        }
    }

    /// Checks if a backup is currently in progress
    pub fn is_backup_in_progress(&self) -> bool {
        matches!(self.state, BackupState::InProgress { .. })
    }

    /// Gets the duration since the last successful backup
    pub fn time_since_last_success(&self) -> Option<Duration> {
        self.last_successful_backup
            .map(|ts| (Utc::now() - ts).to_std().unwrap_or(Duration::from_secs(0)))
    }

    /// Adds a result to the history, maintaining the size limit
    fn add_to_history(&mut self, result: BackupResult) {
        if self.history.len() >= MAX_HISTORY_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(result);
    }

    /// Gets the average backup duration from successful backups in history
    pub fn average_backup_duration(&self) -> Option<Duration> {
        let successful_durations: Vec<Duration> = self
            .history
            .iter()
            .filter(|r| r.success)
            .map(|r| r.duration)
            .collect();

        if successful_durations.is_empty() {
            None
        } else {
            let total: Duration = successful_durations.iter().sum();
            Some(total / successful_durations.len() as u32)
        }
    }

    /// Gets the success rate as a percentage (0.0 - 100.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            (self.success_count as f64 / total as f64) * 100.0
        }
    }
}

/// Thread-safe backup status tracker
pub type SharedBackupStatus = Arc<Mutex<BackupStatus>>;

/// Creates a new shared backup status instance
pub fn create_shared_status() -> SharedBackupStatus {
    Arc::new(Mutex::new(BackupStatus::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_backup_lifecycle() {
        let mut status = BackupStatus::new();

        // Initial state should be idle
        assert_eq!(status.state, BackupState::Idle);
        assert_eq!(status.success_count, 0);
        assert_eq!(status.failure_count, 0);

        // Start a backup
        assert!(status.start_backup());
        assert!(status.is_backup_in_progress());

        // Cannot start another backup while one is in progress
        assert!(!status.start_backup());

        // Complete the backup
        status.complete_backup(1024);
        assert!(!status.is_backup_in_progress());
        assert_eq!(status.success_count, 1);
        assert!(status.last_successful_backup.is_some());

        // Verify state
        match &status.state {
            BackupState::Completed { size_bytes, .. } => {
                assert_eq!(*size_bytes, 1024);
            }
            _ => panic!("Expected Completed state"),
        }
    }

    #[test]
    fn test_backup_failure() {
        let mut status = BackupStatus::new();

        // Start and fail a backup
        assert!(status.start_backup());
        status.fail_backup("Test error".to_string());

        assert_eq!(status.failure_count, 1);
        assert_eq!(status.success_count, 0);
        assert!(status.last_successful_backup.is_none());

        // Verify state
        match &status.state {
            BackupState::Failed { error, .. } => {
                assert_eq!(error, "Test error");
            }
            _ => panic!("Expected Failed state"),
        }
    }

    #[test]
    fn test_history_management() {
        let mut status = BackupStatus::new();

        // Add multiple backups
        for i in 0..10 {
            assert!(status.start_backup());
            thread::sleep(StdDuration::from_millis(10));
            if i % 2 == 0 {
                status.complete_backup(1024 * (i + 1) as u64);
            } else {
                status.fail_backup(format!("Error {}", i));
            }
        }

        assert_eq!(status.history.len(), 10);
        assert_eq!(status.success_count, 5);
        assert_eq!(status.failure_count, 5);
        assert_eq!(status.success_rate(), 50.0);
    }

    #[test]
    fn test_history_size_limit() {
        let mut status = BackupStatus::new();

        // Add more than MAX_HISTORY_SIZE backups
        for i in 0..150 {
            assert!(status.start_backup());
            status.complete_backup(1024 * i);
        }

        assert_eq!(status.history.len(), MAX_HISTORY_SIZE);
        assert_eq!(status.success_count, 150);
    }

    #[test]
    fn test_average_duration() {
        let mut status = BackupStatus::new();

        // Add some successful backups with known durations
        for _ in 0..3 {
            assert!(status.start_backup());
            thread::sleep(StdDuration::from_millis(100));
            status.complete_backup(1024);
        }

        let avg = status.average_backup_duration();
        assert!(avg.is_some());
        // Should be around 100ms, but allow some variance
        let avg_ms = avg.unwrap().as_millis();
        assert!(avg_ms >= 90 && avg_ms <= 150);
    }

    #[test]
    fn test_shared_status_thread_safety() {
        let status = create_shared_status();
        let status_clone = Arc::clone(&status);

        // Try to start backup from two threads
        let handle1 = thread::spawn(move || {
            let mut s = status_clone.lock().unwrap();
            s.start_backup()
        });

        let handle2 = thread::spawn(move || {
            thread::sleep(StdDuration::from_millis(10));
            let mut s = status.lock().unwrap();
            s.start_backup()
        });

        let result1 = handle1.join().unwrap();
        let result2 = handle2.join().unwrap();

        // One should succeed, one should fail
        assert!(result1 != result2);
        assert!(result1 || result2);
    }
}
