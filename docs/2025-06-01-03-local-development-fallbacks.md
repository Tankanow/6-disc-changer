# Local Development Fallbacks for AWS S3

This document describes the local development fallbacks implemented for the SQLite backup/restore system when AWS S3 is unavailable.

## Overview

The 6-Disc Changer application supports backing up its SQLite database to AWS S3 for production deployments. However, for local development, testing, or when AWS credentials are unavailable, the system automatically falls back to using local filesystem storage.

This fallback mechanism ensures that developers can work on the application without requiring AWS credentials or connectivity, while maintaining the same interface and behavior as the production system.

## How It Works

The fallback mechanism works through:

1. A unified `StorageProvider` interface that abstracts away storage details
2. A configuration system that determines when to use AWS vs. local storage
3. Automatic detection of AWS connectivity and credentials
4. A consistent file naming and organization scheme between both implementations

## Configuration

Local fallbacks can be configured through environment variables:

| Environment Variable | Description | Default |
|---------------------|-------------|---------|
| `BACKUP_USE_AWS` | Set to "true" to use AWS when available | `false` |
| `BACKUP_S3_BUCKET` | AWS S3 bucket name | (empty) |
| `AWS_REGION` | AWS region for S3 operations | `us-west-2` |
| `AWS_ROLE_ARN` | AWS IAM role ARN to assume (optional) | (none) |
| `BACKUP_LOCAL_DIR` | Directory for local backups | `./backups` |
| `BACKUP_LOCAL_MAX_COUNT` | Maximum number of local backups to keep | `10` |

## Usage Examples

### Local Development

For local development, you can simply run the application without any AWS-specific configuration:

```bash
cargo run
```

The application will automatically use local filesystem storage for backups.

### Testing with AWS Configuration

To test with AWS configuration but still use local fallbacks:

```bash
export BACKUP_USE_AWS=true
export BACKUP_S3_BUCKET=my-test-bucket
# Omit AWS credentials to force fallback
cargo run
```

The application will detect that AWS credentials are unavailable and fall back to local storage.

## Implementation Details

The local fallback system is implemented in:

- `src/config.rs` - Configuration handling and AWS availability detection
- `src/database/storage/mod.rs` - Storage provider interface and factory
- `src/database/storage/local_storage.rs` - Local filesystem implementation

The system uses a factory pattern to create the appropriate storage provider based on configuration and AWS availability:

```rust
// Create a storage provider (either AWS S3 or local filesystem)
let storage = create_storage_provider(&config).await?;

// Use the storage provider without caring about the implementation
storage.store_backup(&backup_path, &backup_id).await?;
```

## Adding Support for Local Backups to New Code

When implementing new features that use backups:

1. Always use the `StorageProvider` interface rather than direct filesystem or S3 operations
2. Use the `create_storage_provider` factory to get the appropriate provider
3. Handle storage errors appropriately using the `DatabaseError` type

## Troubleshooting

### Local Backups Not Being Created

- Check that the `BACKUP_LOCAL_DIR` directory exists and is writable
- Verify that the application has permission to create files in that directory
- Look for error messages in the logs related to backup operations

### Application Using Local Storage When AWS Should Be Used

- Ensure `BACKUP_USE_AWS` is set to "true"
- Verify that AWS credentials are properly configured
- Check that the S3 bucket name is correctly specified
- Look for AWS connectivity errors in the logs