# Product Requirements Document: SQLite Backup and Restore

## 1. Introduction/Overview

This feature enables automatic backup and restore functionality for the application's SQLite database when running in fly.io containers. Currently, when fly.io starts a new container, the application starts with a fresh database, losing all previously stored data. This feature will decouple the data lifecycle from the container lifecycle by implementing an automatic backup system to S3 and restoring the database on container startup.

## 2. Goals

- Ensure data persistence across fly.io machine lifecycles
- Implement automatic, regular SQLite database backups to S3
- Provide automatic database restoration when starting a new container
- Maintain database integrity without affecting application performance
- Establish proper AWS infrastructure with appropriate security measures

## 3. User Stories

- As a developer, I want my application's data to persist across container restarts so that users don't experience data loss.
- As a developer, I want to use SQLite as a production database with confidence that data will be durable.
- As a developer, I want the backup process to be automatic so that I don't need to manually manage it.
- As a developer, I want my application to automatically restore the most recent database backup when starting in a new container.
- As a developer, I want to be able to run and test the app locally even if AWS or other cloud services are unavailable.

## 4. Functional Requirements

1. The system must automatically back up the SQLite database to S3 every 5 minutes.
2. The system must perform a backup before the container shuts down.
3. The system must automatically restore the most recent database backup when starting from a fresh container.
4. The system must use AWS IAM roles via fly.io OpenID Connect for authentication with S3.
5. The system must write new backup files for each backup operation (rather than overwriting).
6. The system must efficiently identify and retrieve the latest backup during restoration.
7. The system must perform backups without blocking normal application requests.
8. The system must restore the database as quickly as possible during application startup.
9. The system must provide appropriate error handling and logging for backup and restore operations.
10. The system must include infrastructure as code (IaC) via GitHub Actions to provision the required S3 bucket and IAM roles.

## 5. Non-Goals (Out of Scope)

- Handling multiple concurrent database instances with data merging
- Manual backup initiation or restoration of specific backup versions
- Backup compression or differential backups
- Custom retention policies (will rely on S3 bucket lifecycle policies)
- Point-in-time recovery
- Database sharding or complex replication scenarios

## 6. Design Considerations

- The backup and restore processes should be transparent to end users
- The system should be designed to minimize any impact on application performance
- Error states and recovery procedures should be clearly defined
- Logs should provide clear information about backup/restore status

## 7. Technical Considerations

- Use AWS SDK for S3 interaction
- Leverage fly.io's OpenID Connect implementation for AWS authentication
- Consider using a timestamp-based naming convention for backup files
- Implement proper locking mechanisms to ensure database consistency during backup
- Use S3 server-side encryption for data at rest
- Ensure the S3 bucket has no public access
- Define appropriate IAM policies that follow the principle of least privilege

### AWS Infrastructure Requirements

- S3 bucket with:
  - Server-side encryption enabled
  - Public access blocked
  - Appropriate IAM policies
- IAM roles with:
  - Read/write permissions to the designated S3 bucket
  - Trust relationship with the fly.io OpenID Connect provider

## 8. Success Metrics

- Zero data loss during container restarts
- Minimal startup time impact during database restoration (target: <30 seconds)
- No degradation in application performance during backup operations
- Successful implementation of the AWS infrastructure via GitHub Actions

## 9. Open Questions

- What is the expected size growth of the database over time?
- Should we implement monitoring or alerting for backup failures?
- Are there any specific requirements for backup naming conventions?
- Do we need to consider database migration scenarios in the future?
- What is the appropriate timeout value for backup operations?
