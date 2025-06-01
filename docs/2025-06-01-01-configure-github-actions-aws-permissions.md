# Setting up AWS IAM OIDC with GitHub Actions

This guide explains how to configure AWS to trust GitHub as an OIDC (OpenID Connect) identity provider, allowing GitHub Actions workflows to assume IAM roles without storing long-term AWS credentials.

## Prerequisites

- AWS account with administrator access
- GitHub repository with Actions enabled
- Permissions to create IAM roles and identity providers in AWS
- AWS CLI installed and configured (for automated setup)

## Setup Options

There are two ways to set up the AWS IAM OIDC integration:

1. **Automated Setup** - Using a script to automate the entire process with AWS CLI
2. **Manual Setup** - Using the AWS Management Console

## Option 1: Automated Setup with Script

We provide a bash script that automates the entire setup process using the AWS CLI.

### Using the Setup Script

1. **Make the script executable**:
   ```bash
   chmod +x scripts/setup-github-oidc.sh
   ```

2. **Run the script with required parameters**:
   ```bash
   ./scripts/setup-github-oidc.sh \
     --account-id 123456789012 \
     --org your-github-username \
     --repo 6-disc-changer
   ```

3. **Script parameters**:
   - `-a, --account-id` - Your AWS Account ID (required)
   - `-r, --region` - AWS Region (default: us-east-1)
   - `-o, --org` - GitHub Organization or Username (required)
   - `-p, --repo` - GitHub Repository Name (required)
   - `-b, --branch` - Restrict to a specific branch (optional)
   - `-n, --role-name` - Name for the IAM role (default: github-actions-role)

4. **Example with all parameters**:
   ```bash
   ./scripts/setup-github-oidc.sh \
     --account-id 123456789012 \
     --region us-west-2 \
     --org your-github-username \
     --repo 6-disc-changer \
     --branch main \
     --role-name my-custom-github-role
   ```

5. **Follow the script output** to add the required secrets to your GitHub repository

### What the Script Does

The script:
1. Creates an OIDC provider for GitHub if it doesn't exist
2. Creates an IAM role with a trust policy for your GitHub repository
3. Creates and attaches a policy for S3 access to the role
4. Outputs the necessary information to configure GitHub Actions

## Option 2: Manual Setup

The following steps guide you through the manual setup process using the AWS Management Console.

## Step 1: Create an OIDC Identity Provider in AWS

1. **Sign in to the AWS Management Console**
   - Navigate to the IAM service

2. **Create Identity Provider**
   - In the IAM console, click on "Identity providers" in the left navigation panel
   - Click "Add provider"
   - For "Provider type", select "OpenID Connect"
   - For "Provider URL", enter: `https://token.actions.githubusercontent.com`
   - Click "Get thumbprint" to automatically fetch the server certificate thumbprint
   - For "Audience", enter: `sts.amazonaws.com`
   - Click "Add provider"

## Step 2: Create an IAM Role for GitHub Actions

1. **Create a new IAM Role**
   - In the IAM console, click on "Roles" in the left navigation panel
   - Click "Create role"
   - Select "Web identity" as the trusted entity type
   - For "Identity provider", select the GitHub provider you just created (`token.actions.githubusercontent.com`)
   - For "Audience", select `sts.amazonaws.com`
   - Click "Next: Permissions"

2. **Add Permissions to the Role**
   - Search for and select the policy `AmazonS3FullAccess` (or create a custom policy with only the permissions needed)
   - Click "Next: Tags"
   - (Optional) Add tags as needed
   - Click "Next: Review"
   - Enter a name for the role, e.g., `GitHubActionsS3Role`
   - Add a description, e.g., "Role for GitHub Actions to access S3 resources"
   - Click "Create role"

## Step 3: Configure the Trust Relationship

1. **Edit the Trust Relationship**
   - Find and select the role you just created
   - Click on the "Trust relationships" tab
   - Click "Edit trust policy"
   - Replace the policy with the following, updating the repository and branch information:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "arn:aws:iam::ACCOUNT_ID:oidc-provider/token.actions.githubusercontent.com"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "token.actions.githubusercontent.com:aud": "sts.amazonaws.com"
        },
        "StringLike": {
          "token.actions.githubusercontent.com:sub": "repo:ORGANIZATION_NAME/6-disc-changer:*"
        }
      }
    }
  ]
}
```

Replace:
- `ACCOUNT_ID` with your AWS account ID
- `ORGANIZATION_NAME` with your GitHub organization/username

2. **Restrict to Specific Branches or Environments (Optional)**
   - For increased security, you can restrict the role to specific branches or environments:

```json
"token.actions.githubusercontent.com:sub": "repo:ORGANIZATION_NAME/6-disc-changer:ref:refs/heads/main"
```

## Step 4: Configure GitHub Repository Secrets

1. **Add Role ARN as a Secret**
   - In your GitHub repository, go to "Settings" > "Secrets and variables" > "Actions"
   - Click
 "New repository secret"
   - Name: `AWS_ROLE_ARN`
   - Value: `arn:aws:iam::ACCOUNT_ID:role/GitHubActionsS3Role` (replace with your actual role ARN)
   - Click "Add secret"

2. **Add AWS Region as a Secret**
   - Click "New repository secret"
   - Name: `AWS_REGION`
   - Value: `us-west-2` (or your preferred region)
   - Click "Add secret"

3. **Add AWS Account ID as a Secret**
   - Click "New repository secret"
   - Name: `AWS_ACCOUNT_ID`
   - Value: Your AWS account ID
   - Click "Add secret"

## Step 5: Update GitHub Actions Workflow

The GitHub Actions workflow is already configured to use the OIDC provider with the following configuration:

```yaml
- name: Configure AWS credentials
  uses: aws-actions/configure-aws-credentials@v4
  with:
    role-to-assume: ${{ secrets.AWS_ROLE_ARN }}
    aws-region: ${{ secrets.AWS_REGION }}
```

## Step 6: Test the Configuration

1. **Push a change to the repository** to trigger the GitHub Actions workflow
2. **Monitor the workflow execution** in the "Actions" tab of your repository
3. **Check the AWS CloudTrail logs** to verify that the role was successfully assumed

## Security Considerations

1. **Use Conditional Role Assumption**
   - Always use conditions in your trust policy to restrict which repositories and workflows can assume the role

2. **Apply Least Privilege**
   - Grant only the permissions necessary for your workflow to function

3. **Monitor Role Usage**
   - Regularly review CloudTrail logs for role assumption events
   - Set up CloudWatch Alarms for suspicious activity

4. **Rotate GitHub OIDC Thumbprints**
   - Periodically check if GitHub has rotated their OIDC thumbprints and update your identity provider accordingly

## Troubleshooting

1. **Permission Denied Errors**
   - Check that the trust relationship is correctly configured
   - Verify that the GitHub repository name and workflow paths match the conditions in the trust policy

2. **Unable to Assume Role**
   - Ensure the GitHub Actions workflow has the correct permissions block:
     ```yaml
     permissions:
       id-token: write
       contents: read
     ```

3. **OIDC Token Issues**
   - Verify that the audience (`aud`) claim in the GitHub OIDC token matches what's expected in your trust policy

4. **Script Errors**
   - Make sure you have the AWS CLI installed and configured with appropriate permissions
   - Check that you've provided all required parameters to the script
   - If the script fails to get the thumbprint, you can manually specify it with:
     ```
     THUMBPRINT=$(echo | openssl s_client -connect token.actions.githubusercontent.com:443 2>&1 | 
                  sed -ne '/-BEGIN CERTIFICATE-/,/-END CERTIFICATE-/p' | 
                  openssl x509 -fingerprint -noout | 
                  sed 's/://g' | sed 's/.*=//')
     ```

## Resources

- [AWS Security Blog: Use IAM roles to connect GitHub Actions to AWS](https://aws.amazon.com/blogs/security/use-iam-roles-to-connect-github-actions-to-actions-in-aws/)
- [GitHub Docs: Configuring OpenID Connect in AWS](https://docs.github.com/en/actions/deployment/security-hardening-your-deployments/configuring-openid-connect-in-amazon-web-services)