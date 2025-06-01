# AWS Infrastructure Setup and Access Patterns

This document provides a comprehensive guide to the AWS infrastructure used for the 6-Disc Changer SQLite backup and restore system, including its architecture, setup process, and access patterns.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [AWS Resources](#aws-resources)
3. [Environment Configuration](#environment-configuration)
4. [Access Patterns](#access-patterns)
5. [Security Considerations](#security-considerations)
6. [Setup Instructions](#setup-instructions)
7. [Monitoring and Maintenance](#monitoring-and-maintenance)
8. [Troubleshooting](#troubleshooting)

## Architecture Overview

The 6-Disc Changer application uses AWS infrastructure for secure storage and retrieval of SQLite database backups. The architecture follows these design principles:

- **Secure by default**: All resources are configured with encryption and restricted access
- **Least privilege**: IAM roles have only the permissions necessary for their function
- **Infrastructure as Code**: All AWS resources are defined and managed using Pulumi
- **Cross-region support**: Infrastructure can be deployed to multiple regions
- **Identity federation**: Authentication uses OpenID Connect (OIDC) rather than long-lived credentials

The architecture includes these main components:

1. **S3 Bucket**: Stores encrypted database backups
2. **IAM Roles**: Controls access to the S3 bucket
3. **OIDC Integration**: Allows identity federation from fly.io and GitHub Actions

```
┌───────────────┐       ┌──────────────┐       ┌────────────────┐
│ 6-Disc Changer│       │  AWS IAM     │       │                │
│ Application   │─OIDC─▶│  Role        │──────▶│  S3 Bucket     │
│ (on fly.io)   │       │              │       │  (Encrypted)   │
└───────────────┘       └──────────────┘       └────────────────┘
       ▲                        ▲                      ▲
       │                        │                      │
       │                        │                      │
┌──────┴──────┐        ┌───────┴──────┐       ┌───────┴──────┐
│ GitHub     │        │ Pulumi       │       │ Local         │
│ Actions    │        │ Deployment   │       │ Development   │
│ Workflow   │        │ Pipeline     │       │ Fallback      │
└─────────────┘        └──────────────┘       └──────────────┘
```

## AWS Resources

### S3 Bucket

The S3 bucket is the primary storage location for database backups:

- **Naming**: `6-disc-changer-{env}-sqlite-backups`
- **Access**: Private, with no public access allowed
- **Encryption**: AES-256 server-side encryption enabled by default
- **Ownership Controls**: BucketOwnerPreferred ensures objects are owned by the bucket owner
- **Lifecycle Policies**: Not currently configured, but could be added to automatically expire old backups

### IAM Role

A dedicated IAM role with least privilege permissions:

- **Name**: `six-disc-changer-backup-restore-{env}`
- **Permissions**: Limited to specific S3 operations on the backup bucket:
  - `s3:ListBucket` on the bucket itself
  - `s3:GetObject`, `s3:PutObject`, and `s3:DeleteObject` on bucket objects
- **Trust Relationship**: Configured to trust the fly.io OIDC provider

### OIDC Integration

Two OIDC providers are configured:

1. **fly.io OIDC Provider**:
   - Allows the application running on fly.io to assume the IAM role
   - Uses conditions to restrict access to specific fly.io applications

2. **GitHub Actions OIDC Provider** (setup via script):
   - Allows GitHub Actions workflows to assume a role for infrastructure deployment
   - Restricted to specific repositories and branches

## Environment Configuration

The infrastructure supports multiple environments through Pulumi stacks:

### Development Environment

- **Stack**: `dev`
- **Region**: `us-west-2`
- **Bucket**: `6-disc-changer-dev-sqlite-backups`
- **Configuration**: `Pulumi.dev.yaml`

### Production Environment

- **Stack**: `prod`
- **Region**: `us-east-1`
- **Bucket**: `6-disc-changer-prod-sqlite-backups`
- **Configuration**: `Pulumi.prod.yaml`

Both environments use the same infrastructure code but with environment-specific configurations.

## Access Patterns

### Application Access (fly.io)

The application running on fly.io accesses AWS resources using the following pattern:

1. The application requests a JWT token from the fly.io OIDC provider
2. The application calls AWS STS AssumeRoleWithWebIdentity with:
   - The JWT token
   - The target IAM role ARN (`AWS_ROLE_ARN` environment variable)
3. AWS validates the token against the trust policy
4. If valid, AWS returns temporary credentials (access key, secret key, session token)
5. The application uses these credentials to access the S3 bucket
6. Credentials automatically expire after a short period (typically 1 hour)

```
┌────────────────┐     ┌─────────────┐     ┌────────────────┐
│  Application   │     │ fly.io OIDC │     │   AWS STS      │
│  on fly.io     │────▶│  Provider   │────▶│   Service      │
└────────────────┘     └─────────────┘     └────────────────┘
        │                                           │
        │                                           │
        │                                           ▼
        │                                  ┌────────────────┐
        │                                  │ Temporary AWS  │
        │◀─────────────────────────────────│ Credentials    │
        │                                  └────────────────┘
        │
        ▼
┌────────────────┐
│   S3 Bucket    │
│   Operations   │
└────────────────┘
```

### Infrastructure Deployment Access (GitHub Actions)

GitHub Actions workflows access AWS resources using a similar pattern:

1. GitHub generates an OIDC token with claims about the repository and workflow
2. The GitHub Actions workflow uses the `aws-actions/configure-aws-credentials` action
3. The action exchanges the OIDC token for temporary AWS credentials
4. These credentials are used by Pulumi to deploy infrastructure

### Local Development Access

For local development, the application provides a filesystem fallback:

1. Local configuration sets `BACKUP_USE_AWS=false`
2. The application uses local filesystem storage instead of S3
3. When running Pulumi locally, developers use their own AWS credentials

## Security Considerations

### Data Protection

- **Encryption in Transit**: All communication with AWS APIs uses TLS
- **Encryption at Rest**: S3 server-side encryption with AES-256
- **Access Logging**: Not currently enabled, but can be added to track all S3 operations

### Identity and Access Management

- **No Long-term Credentials**: OIDC federation eliminates the need for storing AWS access keys
- **Conditional Access**: IAM trust policies include conditions to restrict which entities can assume roles
- **Principle of Least Privilege**: IAM policies grant only the permissions needed for specific tasks

### Audit and Compliance

- **AWS CloudTrail**: Records all API calls made to AWS services
- **S3 Access Logs**: Can be enabled to record all requests made to the S3 bucket

## Setup Instructions

### Prerequisites

- AWS account with administrator access
- [Pulumi CLI](https://www.pulumi.com/docs/install/) installed
- [AWS CLI](https://aws.amazon.com/cli/) installed and configured

### Deploying Infrastructure with Pulumi

1. **Clone the repository**:
   ```bash
   git clone https://github.com/your-org/6-disc-changer.git
   cd 6-disc-changer
   ```

2. **Install dependencies**:
   ```bash
   npm install
   ```

3. **Select Pulumi stack**:
   ```bash
   cd pulumi
   pulumi stack select dev  # or prod
   ```

4. **Update configuration** (if needed):
   ```bash
   pulumi config set aws:region us-west-2
   pulumi config set sqlite-backup-restore:backupBucketName your-unique-bucket-name
   pulumi config set sqlite-backup-restore:flyAppName your-fly-app-name
   pulumi config set sqlite-backup-restore:flyOrgSlug your-fly-org
   ```

5. **Deploy infrastructure**:
   ```bash
   pulumi up
   ```

6. **Note the outputs**:
   - `bucketName`: The name of the created S3 bucket
   - `backupRestoreRoleArn`: The ARN of the IAM role for the application

### Setting up fly.io Application

1. **Add AWS role ARN to fly.io configuration**:
   ```bash
   fly secrets set AWS_ROLE_ARN=arn:aws:iam::ACCOUNT_ID:role/six-disc-changer-backup-restore-dev
   ```

2. **Enable backups in application configuration**:
   ```bash
   fly secrets set BACKUP_USE_AWS=true
   fly secrets set BACKUP_S3_BUCKET=your-bucket-name
   ```

### Setting up GitHub Actions

1. **Run the OIDC setup script**:
   ```bash
   ./scripts/setup-github-oidc.sh \
     --account-id YOUR_AWS_ACCOUNT_ID \
     --org YOUR_GITHUB_ORG \
     --repo 6-disc-changer
   ```

2. **Add GitHub repository secrets**:
   - `AWS_ROLE_ARN`: The ARN of the role created by the script
   - `AWS_REGION`: The AWS region to use
   - `AWS_ACCOUNT_ID`: Your AWS account ID
   - `PULUMI_CONFIG_PASSPHRASE`: A passphrase for Pulumi configuration encryption

## Monitoring and Maintenance

### Monitoring AWS Resources

1. **CloudWatch Metrics**:
   - S3 bucket metrics for storage and request counts
   - CloudTrail metrics for API calls

2. **CloudWatch Alarms**:
   - Set up alarms for unusual access patterns
   - Monitor for high error rates

### Cost Management

- **S3 Storage Costs**: Depends on the size and frequency of backups
- **S3 Data Transfer**: Minimal for most operations
- **AWS CloudTrail**: Free for management events

### Maintenance Tasks

- **Rotate OIDC Thumbprints**: Check periodically if fly.io or GitHub have rotated their certificates
- **Review IAM Policies**: Regularly audit permissions to ensure they follow least privilege
- **Check S3 Bucket Policy**: Verify no unintended changes have been made

## Troubleshooting

### Common Issues

1. **Access Denied Errors**:
   - Verify the trust relationship in the IAM role
   - Check that the OIDC token subject matches the conditions in the trust policy
   - Ensure the IAM policy allows the specific S3 operations being attempted

2. **OIDC Token Issues**:
   - Verify the OIDC provider URL is correct
   - Check that the audience claim matches what's expected

3. **S3 Bucket Errors**:
   - Ensure the bucket name matches what's configured in the application
   - Verify the bucket exists in the expected region

### Debugging Tools

- **AWS CloudTrail**: Review API calls to identify permission issues
- **IAM Policy Simulator**: Test IAM policies before deployment
- **AWS CLI**: Test S3 operations with temporary credentials:
  ```bash
  aws sts assume-role-with-web-identity \
    --role-arn $ROLE_ARN \
    --role-session-name test-session \
    --web-identity-token $TOKEN
  ```

### Support Resources

- [AWS S3 Documentation](https://docs.aws.amazon.com/s3/)
- [AWS IAM Documentation](https://docs.aws.amazon.com/iam/)
- [fly.io Documentation](https://fly.io/docs/)
- [Pulumi Documentation](https://www.pulumi.com/docs/)