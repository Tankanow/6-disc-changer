# Container Lifecycle Management

This document describes how the 6-disc-changer application handles container lifecycle events, particularly focusing on graceful shutdown and data persistence during container termination.

## Overview

When deployed on fly.io or other container platforms, applications receive termination signals (SIGTERM) before being shut down. This happens during:
- Application deployments
- Scaling operations
- Platform maintenance
- Manual restarts

The 6-disc-changer implements graceful shutdown handling to ensure data integrity by performing a final database backup before the container terminates.

## Shutdown Process

### 1. Signal Handling

The application listens for two termination signals:
- **SIGINT** (Ctrl+C): Used during local development
- **SIGTERM**: Sent by container orchestrators like fly.io

When either signal is received, the shutdown process begins.

### 2. Shutdown Sequence

The shutdown sequence follows these steps:

1. **Signal Reception**: The signal handler detects SIGTERM/SIGINT
2. **Shutdown Initiation**: The `ShutdownManager` begins the shutdown process
3. **Scheduler Termination**: The backup scheduler is gracefully stopped
4. **Final Backup**: A special "shutdown" backup is triggered
5. **Server Shutdown**: The HTTP server stops accepting new connections
6. **Graceful Completion**: Existing requests complete before final termination

### 3. Shutdown Backup

The shutdown backup has special characteristics:
- **Naming Convention**: Uses `BackupType::Shutdown` to distinguish from scheduled backups
- **Performance Optimized**: 
  - Larger chunk size (128 pages vs 64)
  - No sleep between chunks
  - Verification skipped for speed
- **Timeout Protection**: 30-second timeout ensures shutdown isn't delayed indefinitely
- **Lock Handling**: 5-second timeout on acquiring backup mutex

## Implementation Details

### Key Components

1. **`ShutdownManager`** (`src/shutdown.rs`)
   - Coordinates the shutdown process
   - Manages shutdown state to prevent duplicate shutdowns
   - Triggers the final backup

2. **Signal Handlers** 
   - Platform-agnostic SIGINT handling
   - Unix-specific SIGTERM handling
   - Integrated with tokio's signal handling

3. **Backup Integration**
   - `perform_shutdown_backup()` method in `BackupManager`
   - Special backup naming with timestamp and type

### Configuration

No special configuration is required. The shutdown backup uses the same storage configuration as regular backups:
- S3 storage in production
- Local storage fallback in development

## fly.io Specific Considerations

### Termination Grace Period

fly.io provides a grace period (typically 30 seconds) between sending SIGTERM and forcefully killing the container. The shutdown backup is designed to complete within this window.

### Health Checks

During shutdown:
1. The HTTP server continues responding until the shutdown process completes
2. New connections are rejected gracefully
3. Existing connections are allowed to complete

### Deployment Best Practices

1. **Rolling Deployments**: fly.io's default rolling deployment ensures zero downtime
2. **Multiple Instances**: Running multiple instances prevents data loss if one instance fails during shutdown
3. **Monitoring**: Check fly.io logs for shutdown backup success messages

## Troubleshooting

### Common Issues

1. **Shutdown Backup Timeout**
   - **Symptom**: "Shutdown backup timed out after 30 seconds" in logs
   - **Cause**: Large database or slow S3 connection
   - **Solution**: Optimize database size or increase chunk size

2. **Backup Mutex Timeout**
   - **Symptom**: "Failed to acquire backup mutex for shutdown backup - timeout"
   - **Cause**: Another backup is running and taking too long
   - **Solution**: Reduce backup frequency or optimize backup performance

3. **Missing Shutdown Backups**
   - **Symptom**: No shutdown backups in S3
   - **Cause**: Container killed before backup completes
   - **Solution**: Check fly.io termination grace period settings

### Log Messages

Key log messages to monitor:

```
INFO: "Initiating graceful shutdown sequence"
INFO: "Starting shutdown backup"
INFO: "Shutdown backup completed successfully"
ERROR: "Shutdown backup failed: <error>"
WARN: "Shutdown backup timed out after 30 seconds"
```

### Verification

To verify shutdown backups are working:

1. Check S3 for backups with "shutdown" in the filename:
   ```
   backup_2025-06-01_143000_prod_server1_shutdown_abcdef.db
   ```

2. Monitor fly.io deployment logs:
   ```bash
   fly logs --app your-app-name | grep shutdown
   ```

3. Test locally:
   ```bash
   cargo run
   # Press Ctrl+C and check for shutdown backup logs
   ```

## Recovery Scenarios

### Incomplete Shutdown

If a container is forcefully terminated:
1. The last scheduled backup will be used for restoration
2. Data loss is limited to the time since the last successful backup
3. The backup scheduler runs every 5 minutes by default

### Corruption Detection

On startup, the restoration checker will:
1. Detect if the database is missing or corrupted
2. Automatically restore from the latest backup
3. Prefer shutdown backups over scheduled backups when timestamps are equal

## Best Practices

1. **Monitor Shutdown Duration**: Ensure shutdown completes within platform limits
2. **Test Shutdown Locally**: Verify shutdown backups work in development
3. **Set Appropriate Intervals**: Balance backup frequency with performance
4. **Use Multiple Instances**: Prevent single points of failure
5. **Monitor S3 Storage**: Ensure adequate space and permissions

## Related Documentation

- [AWS Infrastructure Setup](./2025-06-01-04-aws-infrastructure-setup-access-patterns.md)
- [Local Development Fallbacks](./2025-06-01-03-local-development-fallbacks.md)
- [fly.io OIDC Setup](./2025-06-01-02-fly-io-oidc-setup.md)