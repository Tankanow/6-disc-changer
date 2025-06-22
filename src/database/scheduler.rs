//! Backup scheduler module for managing automated database backups
//!
//! This module provides functionality to schedule regular backups
//! with proper status tracking and concurrency control.

use crate::database::Result;
use crate::database::backup::{BackupManager, BackupOptions};
use crate::database::backup_status::SharedBackupStatus;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Manages scheduled backup operations
pub struct BackupScheduler {
    /// Backup manager instance
    backup_manager: Arc<BackupManager>,
    /// Backup status tracker
    status: SharedBackupStatus,
    /// Handle to the scheduler task
    scheduler_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// Whether the scheduler is running
    is_running: Arc<RwLock<bool>>,
    /// Notification for stopping the scheduler
    stop_notify: Arc<Notify>,
}

impl BackupScheduler {
    /// Create a new backup scheduler
    pub fn new(backup_manager: Arc<BackupManager>, status: SharedBackupStatus) -> Self {
        Self {
            backup_manager,
            status,
            scheduler_handle: Arc::new(RwLock::new(None)),
            is_running: Arc::new(RwLock::new(false)),
            stop_notify: Arc::new(Notify::new()),
        }
    }

    /// Start the backup scheduler with the specified interval
    pub async fn start(&self, interval: Duration, options: BackupOptions) -> Result<()> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            warn!("Backup scheduler is already running");
            return Ok(());
        }

        *is_running = true;
        info!("Starting backup scheduler with interval: {:?}", interval);

        let backup_manager = self.backup_manager.clone();
        let status = self.status.clone();
        let is_running_flag = self.is_running.clone();
        let stop_notify = self.stop_notify.clone();

        let handle = tokio::spawn(async move {
            let mut interval_timer = time::interval(interval);
            interval_timer.tick().await; // Skip the first immediate tick

            loop {
                // Wait for either the interval tick or stop notification
                tokio::select! {
                    _ = interval_timer.tick() => {
                        // Check if we should stop
                        if !*is_running_flag.read().await {
                            info!("Backup scheduler stopping");
                            break;
                        }
                    }
                    _ = stop_notify.notified() => {
                        info!("Backup scheduler received stop notification");
                        break;
                    }
                }

                // Check backup status before attempting
                let should_backup = {
                    let status_guard = status.lock().unwrap();
                    if status_guard.is_backup_in_progress() {
                        debug!("Skipping scheduled backup - another backup is in progress");
                        false
                    } else {
                        // Check time since last successful backup
                        if let Some(duration) = status_guard.time_since_last_success() {
                            if duration < interval / 2 {
                                debug!(
                                    "Skipping scheduled backup - last backup was only {:?} ago",
                                    duration
                                );
                                false
                            } else {
                                true
                            }
                        } else {
                            // No successful backups yet
                            true
                        }
                    }
                };

                if should_backup {
                    info!("Starting scheduled backup");
                    match backup_manager.create_backup(options.clone()).await {
                        Ok(job_id) => {
                            info!("Scheduled backup started with job ID: {}", job_id);
                        }
                        Err(e) => {
                            error!("Failed to start scheduled backup: {}", e);
                        }
                    }
                }
            }
        });

        let mut handle_guard = self.scheduler_handle.write().await;
        *handle_guard = Some(handle);

        Ok(())
    }

    /// Stop the backup scheduler gracefully
    pub async fn stop(&self) {
        info!("Stopping backup scheduler");

        // Set running flag to false
        let mut is_running = self.is_running.write().await;
        if !*is_running {
            debug!("Backup scheduler is already stopped");
            return;
        }
        *is_running = false;

        // Notify the scheduler task to stop
        self.stop_notify.notify_waiters();

        // Wait for the scheduler task to complete
        let mut handle_guard = self.scheduler_handle.write().await;
        if let Some(handle) = handle_guard.take() {
            // Give it time to stop gracefully
            if tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .is_err()
            {
                warn!("Backup scheduler task did not stop gracefully, aborting");
                // The handle is already consumed by the timeout, nothing more to do
            } else {
                debug!("Backup scheduler task stopped gracefully");
            }
        }
    }

    /// Trigger an immediate backup outside the regular schedule
    pub async fn trigger_immediate_backup(&self, options: BackupOptions) -> Result<String> {
        info!("Triggering immediate backup from scheduler");

        // Check if scheduler is running
        if !*self.is_running.read().await {
            warn!("Cannot trigger backup - scheduler is not running");
            return Err(crate::database::DatabaseError::BackupServiceUnavailable);
        }

        // Delegate to backup manager
        self.backup_manager.create_backup(options).await
    }

    /// Check if the scheduler is currently running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    /// Get the backup status tracker
    pub fn get_status(&self) -> &SharedBackupStatus {
        &self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackupConfig;
    use crate::database::backup_status::create_shared_status;
    use crate::database::storage::local_storage::LocalStorageProvider;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::PathBuf;
    use tempfile::TempDir;

    async fn create_test_backup_manager() -> (Arc<BackupManager>, SharedBackupStatus, TempDir) {
        let temp_dir = TempDir::new().unwrap();

        // Create a test database in memory
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Create test config
        let config = BackupConfig {
            local_backup_dir: PathBuf::from(temp_dir.path().join("backups")),
            ..Default::default()
        };

        let storage = Arc::new(LocalStorageProvider::new(&config));
        let status = create_shared_status();
        let manager = Arc::new(BackupManager::new(
            pool,
            storage,
            "test",
            Some("test-server"),
            status.clone(),
        ));

        (manager, status, temp_dir)
    }

    #[tokio::test]
    async fn test_scheduler_lifecycle() {
        let (manager, status, _temp_dir) = create_test_backup_manager().await;
        let scheduler = BackupScheduler::new(manager, status);

        // Initially not running
        assert!(!scheduler.is_running().await);

        // Start the scheduler
        let options = BackupOptions::default();
        scheduler
            .start(Duration::from_secs(60), options)
            .await
            .unwrap();
        assert!(scheduler.is_running().await);

        // Cannot start again while running
        let result = scheduler
            .start(Duration::from_secs(60), BackupOptions::default())
            .await;
        assert!(result.is_ok()); // Should succeed but log a warning

        // Stop the scheduler
        scheduler.stop().await;
        assert!(!scheduler.is_running().await);
    }

    #[tokio::test]
    async fn test_scheduler_respects_status() {
        let (manager, status, _temp_dir) = create_test_backup_manager().await;

        // Manually set backup in progress
        {
            let mut status_guard = status.lock().unwrap();
            status_guard.start_backup();
        }

        let scheduler = BackupScheduler::new(manager, status.clone());

        // Start scheduler with very short interval
        scheduler
            .start(Duration::from_millis(100), BackupOptions::default())
            .await
            .unwrap();

        // Wait a bit to ensure scheduler would have tried to backup
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Check that no new backup was started (status should still show in progress)
        {
            let status_guard = status.lock().unwrap();
            assert!(status_guard.is_backup_in_progress());
        }

        scheduler.stop().await;
    }

    #[tokio::test]
    async fn test_immediate_backup_trigger() {
        let (manager, status, _temp_dir) = create_test_backup_manager().await;
        let scheduler = BackupScheduler::new(manager, status);

        // Start the scheduler
        scheduler
            .start(Duration::from_secs(60), BackupOptions::default())
            .await
            .unwrap();

        // Trigger immediate backup
        let result = scheduler
            .trigger_immediate_backup(BackupOptions::default())
            .await;
        assert!(result.is_ok());

        scheduler.stop().await;
    }
}
