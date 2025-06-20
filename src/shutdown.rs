use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::database::backup::BackupManager;
use crate::database::scheduler::BackupScheduler;

/// Manages graceful shutdown of the application
pub struct ShutdownManager {
    /// Notification channel for shutdown signal
    notify: Arc<Notify>,
    /// Track if shutdown has been initiated
    is_shutting_down: Arc<Mutex<bool>>,
    /// Reference to backup manager for final backup
    backup_manager: Arc<BackupManager>,
    /// Reference to scheduler to stop scheduled backups
    scheduler: Arc<RwLock<Option<Arc<BackupScheduler>>>>,
}

impl ShutdownManager {
    /// Create a new shutdown manager
    pub fn new(backup_manager: Arc<BackupManager>) -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            is_shutting_down: Arc::new(Mutex::new(false)),
            backup_manager,
            scheduler: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the backup scheduler reference
    pub async fn set_scheduler(&self, scheduler: Arc<BackupScheduler>) {
        let mut scheduler_lock = self.scheduler.write().await;
        *scheduler_lock = Some(scheduler);
    }

    /// Get a shutdown signal receiver
    pub fn get_shutdown_signal(&self) -> Arc<Notify> {
        Arc::clone(&self.notify)
    }

    /// Initiate graceful shutdown
    pub async fn initiate_shutdown(&self) {
        let mut is_shutting_down = self.is_shutting_down.lock().await;
        if *is_shutting_down {
            info!("Shutdown already in progress, ignoring duplicate signal");
            return;
        }
        *is_shutting_down = true;
        drop(is_shutting_down);

        info!("Initiating graceful shutdown sequence");

        // Stop the scheduler first to prevent new backups
        {
            let scheduler_lock = self.scheduler.read().await;
            if let Some(scheduler) = &*scheduler_lock {
                info!("Stopping backup scheduler");
                scheduler.stop().await;
            }
        }

        // Perform shutdown backup
        match self.perform_shutdown_backup().await {
            Ok(_) => info!("Shutdown backup completed successfully"),
            Err(e) => error!("Shutdown backup failed: {}", e),
        }

        // Notify all waiters that shutdown is complete
        self.notify.notify_waiters();
    }

    /// Perform a final backup before shutdown
    async fn perform_shutdown_backup(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting shutdown backup");

        // Use a timeout to ensure we don't delay shutdown indefinitely
        let backup_timeout = Duration::from_secs(30);

        match timeout(
            backup_timeout,
            self.backup_manager.perform_shutdown_backup(),
        )
        .await
        {
            Ok(Ok(_)) => {
                info!("Shutdown backup completed within timeout");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Shutdown backup failed: {}", e);
                Err(e.into())
            }
            Err(_) => {
                warn!(
                    "Shutdown backup timed out after {} seconds",
                    backup_timeout.as_secs()
                );
                Err("Backup timeout".into())
            }
        }
    }

    /// Wait for shutdown signal
    pub async fn wait_for_shutdown(&self) {
        self.notify.notified().await;
        info!("Shutdown signal received");
    }
}

/// Setup signal handlers for graceful shutdown
pub async fn setup_signal_handlers(shutdown_manager: Arc<ShutdownManager>) {
    use tokio::signal;

    let shutdown_manager_sigterm = Arc::clone(&shutdown_manager);
    let shutdown_manager_sigint = Arc::clone(&shutdown_manager);

    // Handle SIGTERM
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received SIGINT/Ctrl+C signal");
                shutdown_manager_sigint.initiate_shutdown().await;
            }
            Err(err) => {
                error!("Failed to listen for SIGINT signal: {}", err);
            }
        }
    });

    // Handle SIGTERM (Unix only)
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        tokio::spawn(async move {
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to listen for SIGTERM signal: {}", err);
                    return;
                }
            };

            sigterm.recv().await;
            info!("Received SIGTERM signal");
            shutdown_manager_sigterm.initiate_shutdown().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackupConfig;
    use crate::database::backup::BackupManager;
    use crate::database::backup_status::create_shared_status;
    use crate::database::storage::create_storage_provider;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::PathBuf;

    async fn create_test_backup_manager() -> Arc<BackupManager> {
        // Create a test database pool
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        let config = BackupConfig {
            s3_bucket_name: "test-bucket".to_string(),
            aws_region: "us-east-1".to_string(),
            database_path: PathBuf::from("/tmp/test.db"),
            s3_prefix: "test-backups".to_string(),
            backup_interval_seconds: 300,
            use_aws: false,
            ..Default::default()
        };

        let storage = create_storage_provider(&config).await.unwrap();
        let status = create_shared_status();
        Arc::new(BackupManager::new(
            pool,
            storage.into(),
            &config.environment,
            config.server_id.as_deref(),
            status,
        ))
    }

    #[tokio::test]
    async fn test_shutdown_manager_creation() {
        let backup_manager = create_test_backup_manager().await;
        let shutdown_manager = ShutdownManager::new(backup_manager);

        let is_shutting_down = shutdown_manager.is_shutting_down.lock().await;
        assert!(!*is_shutting_down);
    }

    #[tokio::test]
    async fn test_duplicate_shutdown_prevention() {
        let backup_manager = create_test_backup_manager().await;
        let shutdown_manager = Arc::new(ShutdownManager::new(backup_manager));

        // First shutdown
        let manager1 = Arc::clone(&shutdown_manager);
        let handle1 = tokio::spawn(async move {
            manager1.initiate_shutdown().await;
        });

        // Wait a bit to ensure first shutdown starts
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Second shutdown (should be ignored)
        let manager2 = Arc::clone(&shutdown_manager);
        let handle2 = tokio::spawn(async move {
            manager2.initiate_shutdown().await;
        });

        // Both should complete without issues
        handle1.await.unwrap();
        handle2.await.unwrap();

        // Verify shutdown state
        let is_shutting_down = shutdown_manager.is_shutting_down.lock().await;
        assert!(*is_shutting_down);
    }

    #[tokio::test]
    async fn test_shutdown_signal() {
        let backup_manager = create_test_backup_manager().await;
        let shutdown_manager = Arc::new(ShutdownManager::new(backup_manager));

        let signal = shutdown_manager.get_shutdown_signal();

        // Start waiting for shutdown in background
        let signal_clone = Arc::clone(&signal);
        let wait_handle = tokio::spawn(async move {
            signal_clone.notified().await;
        });

        // Initiate shutdown
        shutdown_manager.initiate_shutdown().await;

        // The wait should complete
        tokio::time::timeout(Duration::from_secs(1), wait_handle)
            .await
            .expect("Shutdown signal should be received")
            .expect("Wait task should complete");
    }
}
