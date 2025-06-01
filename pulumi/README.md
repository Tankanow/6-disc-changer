# SQLite Backup Infrastructure

This directory contains [Pulumi YAML](https://www.pulumi.com/docs/yaml/) infrastructure code for setting up AWS resources needed for the SQLite backup and restore system.

## Resources Created

- **S3 Bucket**: For storing SQLite database backups with server-side encryption
- **IAM Role**: With permissions to read/write to the S3 bucket
- **Trust Relationship**: Between the IAM role and fly.io's OpenID Connect provider

## Prerequisites

- [Pulumi CLI](https://www.pulumi.com/docs/install/) installed
- AWS credentials configured
- Fly.io application set up

## Configuration

The infrastructure is configured using stack-specific YAML files:

- `Pulumi.dev.yaml`: Development environment configuration
- `Pulumi.prod.yaml`: Production environment configuration

You should update these files with your specific values:

```yaml
config:
  aws:region: us-east-1  # AWS region to deploy resources
  sqlite-backup-restore:backupBucketName: your-unique-bucket-name
  sqlite-backup-restore:appName: your-fly-app-name
```

## Deployment

To deploy the infrastructure:

```bash
# Select a stack (dev or prod)
pulumi stack select dev

# Preview changes
pulumi preview

# Deploy changes
pulumi up
```

## Outputs

After deployment, Pulumi will output:

- `bucketName`: Name of the created S3 bucket
- `roleArn`: ARN of the IAM role for use in your application
- `region`: AWS region where resources were deployed

Use these values in your application's configuration to enable SQLite backup and restore functionality.

## Local Development

For local development without AWS, the application should provide a fallback mechanism that doesn't rely on these AWS resources.