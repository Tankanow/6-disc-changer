# Setting up fly.io OIDC with AWS IAM

This document explains how to configure AWS IAM to trust fly.io's OpenID Connect (OIDC) identity provider, allowing applications deployed on fly.io to securely access AWS resources without storing long-term credentials.

## How It Works

When using OIDC federation:
1. Your application on fly.io requests a temporary token from the fly.io OIDC provider
2. This token is used to assume an IAM role in AWS via STS (Security Token Service)
3. AWS validates the token against a trust policy
4. If valid, AWS returns temporary credentials that your application can use to access AWS resources

This approach is more secure than storing AWS access keys in your application because:
- No long-term credentials are stored in your application
- Access is temporary and automatically expires
- Permissions are limited to what's defined in the IAM role
- Access can be restricted to specific fly.io applications

## Prerequisites

- An AWS account with permissions to create IAM roles and identity providers
- A fly.io account with at least one application deployed
- Your fly.io organization slug (found in the URL when viewing your org in the dashboard)

## Implementation in Pulumi

Our project uses Pulumi to define and deploy AWS infrastructure, including the OIDC configuration. The implementation includes:

1. Creating an OIDC Identity Provider for fly.io
2. Setting up an IAM Role with a trust policy that allows specific fly.io applications to assume the role
3. Att
aching permissions to the role for accessing S3 buckets

### Key Components in Pulumi.yaml

```yaml
# OIDC Provider for fly.io
flyIoOidcProvider:
  type: aws:iam:OidcProvider
  properties:
    url: "https://oidc.fly.io/${flyOrgSlug}"
    clientIdList:
      - "sts.amazonaws.com"
    thumbprintList:
      - "7e82aecb58c09e2aff05bc8fbc1c46229bfcfb42d5821703f850b7c335cb4685"

# IAM Role with OIDC trust relationship
backupRestoreRole:
  type: aws:iam:Role
  properties:
    name: six-disc-changer-backup-restore-${pulumi.stack}
    description: "Role for 6-disc-changer to perform database backup and restore operations"
    assumeRolePolicy: |
      {
        "Version": "2012-10-17",
        "Statement": [
          {
            "Effect": "Allow",
            "Principal": {
              "Federated": "${flyIoOidcProvider.arn}"
            },
            "Action": "sts:AssumeRoleWithWebIdentity",
            "Condition": {
              "StringEquals": {
                "oidc.fly.io/${flyOrgSlug}:aud": "sts.amazonaws.com"
              },
              "StringLike": {
                "oidc.fly.io/${flyOrgSlug}:sub": "app:${flyAppName}"
              }
            }
          }
        ]
      }
```

### Configuration Parameters

The following parameters need to be set in your Pulumi configuration files:

- `flyOrgSlug`: Your fly.io organization slug
- `flyAppName`: The name of your fly.io application

## Manual Setup

If you're not using Pulumi, you can manually set up the OIDC integration through the AWS Console:

### 1. Create an OIDC Identity Provider

1. Open the AWS Management Console and navigate to IAM
2. In the left navigation pane, choose **Identity providers** > **Add provider**
3. Select **OpenID Connect** as the provider type
4. For **Provider URL**, enter `https://oidc.fly.io/YOUR_FLY_ORG_SLUG`
5. For **Audience**, enter `sts.amazonaws.com`
6. Verify the thumbprint and click **Add provider**

### 2. Create an IAM Role

1. In the IAM console, go to **Roles** > **Create role**
2. Select **Web identity** as the trusted entity type
3. For **Identity provider**, select the fly.io provider you just created
4. For **Audience**, select `sts.amazonaws.com`
5. Add a condition to restrict access to specific applications:
   - Condition: `StringLike`
   - Key: `oidc.fly.io/YOUR_FLY_ORG_SLUG:sub`
   - Value: `app:YOUR_FLY_APP_NAME`
6. Attach the necessary permissions policies (e.g., S3 access for backups)
7. Name the role and create it

## Configuring Your fly.io Application

To use the IAM role from your fly.io application, set the following environment variable in your `fly.toml` file:

```toml
[env]
  AWS_ROLE_ARN = "arn:aws:iam::YOUR_AWS_ACCOUNT_ID:role/YOUR_ROLE_NAME"
```

The AWS SDK will automatically detect this environment variable and use OIDC to assume the role.

## Security Considerations

1. **Scope permissions tightly**: Only grant the minimum permissions needed for your application
2. **Restrict by application**: Use the `sub` claim to limit which fly.io applications can assume the role
3. **Consider adding conditions**: You can add additional conditions to the trust policy, such as restricting by IP range
4. **Monitor role usage**: Set up AWS CloudTrail to monitor role assumption events
5. **Rotate thumbprints**: Periodically check if fly.io has rotated their OIDC thumbprints

## Troubleshooting

If your application cannot assume the role:

1. Verify that the role ARN is correctly set in your application
2. Check that the trust policy conditions match your fly.io organization and application
3. Ensure the role has the necessary permissions to perform the actions your application needs
4. Check CloudTrail logs for any denied actions or invalid assumptions
5. Verify the thumbprint is current

## References

- [fly.io OIDC Documentation](https://fly.io/blog/oidc-cloud-roles/)
- [AWS IAM OIDC Identity Providers](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_oidc.html)
- [AWS STS AssumeRoleWithWebIdentity](https://docs.aws.amazon.com/STS/latest/APIReference/API_AssumeRoleWithWebIdentity.html)