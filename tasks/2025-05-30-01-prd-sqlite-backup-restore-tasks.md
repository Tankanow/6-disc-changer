## Relevant Files

- `src/database/backup.rs` - Handles SQLite database backup operations to S3.
- `src/database/restore.rs` - Handles SQLite database restoration from S3 backups.
- `src/database/s3_client.rs` - AWS S3 client implementation for database operations.
- `src/database/scheduler.rs` - Manages scheduled backup operations.
- `src/config.rs` - Configuration settings for backup/restore functionality.
- `src/main.rs` - Entry point where database restoration happens on startup.
- `.github/workflows/aws-infrastructure.yml` - GitHub Actions workflow for AWS infrastructure provisioning.
- `pulumi/` - Infrastructure as Code for AWS resources using Pulumi YAML.

### Notes

- Unit tests should placed inside the same code files they are testing under a module called `test`.
- Use `cargo test [optional/path/to/test/file]` to run tests. Running without a path executes all tests found by the cargo configuration.

## Tasks

- [ ] 1.0 Set up AWS Infrastructure
  - [ ] 1.1 Create Pulumi YAML project structure for AWS resources
  - [ ] 1.2 Define S3 bucket with server-side encryption and public access blocked
  - [ ] 1.3 Configure IAM roles with least privilege permissions for S3 access
  - [ ] 1.4 Set up trust relationship between IAM roles and fly.io OpenID Connect
  - [ ] 1.5 Implement GitHub Actions workflow to deploy Pulumi infrastructure
  - [ ] 1.6 Create local development fallbacks when AWS is unavailable
  - [ ] 1.7 Document AWS infrastructure setup and access patterns
- [ ] 2.0 Implement SQLite Database Backup System
  - [ ] 2.1 Create S3 client implementation using AWS SDK
  - [ ] 2.2 Implement SQLite backup operations with proper locking mechanisms
  - [ ] 2.3 Design timestamp-based naming convention for backup files
  - [ ] 2.4 Ensure backup operations run in background threads
  - [ ] 2.5 Add configuration options for backup paths and S3 bucket information
  - [ ] 2.6 Implement backup status tracking and result verification
- [ ] 3.0 Implement SQLite Database Restore System
  - [ ] 3.1 Create logic to detect when database restoration is needed
  - [ ] 3.2 Implement efficient algorithm to identify the latest backup in S3
  - [ ] 3.3 Design and implement the database restoration process
  - [ ] 3.4 Add integrity verification for restored databases
  - [ ] 3.5 Implement startup sequence to restore before application begins serving requests
  - [ ] 3.6 Add fallback mechanism for when restoration fails
- [ ] 4.0 Configure Backup Scheduling and Container Lifecycle Hooks
  - [ ] 4.1 Implement scheduler for regular 5-minute backup intervals
  - [ ] 4.2 Add pre-shutdown backup trigger for container termination
  - [ ] 4.3 Configure fly.io container lifecycle hooks
  - [ ] 4.4 Implement graceful handling of backup cancellation during shutdown
  - [ ] 4.5 Add configuration options for backup frequency and timing
- [ ] 5.0 Implement Error Handling, Logging, and Testing
  - [ ] 5.1 Add comprehensive logging for backup and restore operations
  - [ ] 5.2 Implement robust error handling for network and S3 failures
  - [ ] 5.3 Create unit tests for backup and restore functionality
  - [ ] 5.4 Implement integration tests with mock S3 service
  - [ ] 5.5 Develop performance tests to ensure minimal impact on application
  - [ ] 5.6 Document error scenarios and recovery procedures
  - [ ] 5.7 Create monitoring recommendations for production deployments