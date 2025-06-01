#!/bin/bash
set -e

# Script to set up AWS OIDC for GitHub Actions
# This script automates the creation of an OIDC identity provider and IAM role
# to allow GitHub Actions to assume a role in AWS without storing credentials

# Usage instructions
usage() {
  echo "Usage: $0 [options]"
  echo "Options:"
  echo "  -a, --account-id AWS_ACCOUNT_ID     AWS Account ID (required)"
  echo "  -r, --region AWS_REGION             AWS Region (default: us-east-1)"
  echo "  -o, --org GITHUB_ORG                GitHub Organization or Username (required)"
  echo "  -p, --repo GITHUB_REPO              GitHub Repository Name (required)"
  echo "  -b, --branch GITHUB_BRANCH          Restrict to specific branch (default: all branches)"
  echo "  -n, --role-name ROLE_NAME           Name for the IAM role (default: github-actions-role)"
  echo "  -h, --help                          Display this help message"
  exit 1
}

# Default values
AWS_REGION="us-east-1"
ROLE_NAME="github-actions-role"
RESTRICT_BRANCH=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    -a|--account-id)
      AWS_ACCOUNT_ID="$2"
      shift 2
      ;;
    -r|--region)
      AWS_REGION="$2"
      shift 2
      ;;
    -o|--org)
      GITHUB_ORG="$2"
      shift 2
      ;;
    -p|--repo)
      GITHUB_REPO="$2"
      shift 2
      ;;
    -b|--branch)
      GITHUB_BRANCH="$2"
      RESTRICT_BRANCH=true
      shift 2
      ;;
    -n|--role-name)
      ROLE_NAME="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "Unknown option: $1"
      usage
      ;;
  esac
done

# Validate required parameters
if [[ -z $AWS_ACCOUNT_ID || -z $GITHUB_ORG || -z $GITHUB_REPO ]]; then
  echo "Error: Missing required parameters"
  usage
fi

echo "Setting up AWS OIDC for GitHub Actions with the following parameters:"
echo "AWS Account ID: $AWS_ACCOUNT_ID"
echo "AWS Region: $AWS_REGION"
echo "GitHub Organization/User: $GITHUB_ORG"
echo "GitHub Repository: $GITHUB_REPO"
if [[ $RESTRICT_BRANCH == true ]]; then
  echo "Restricting to branch: $GITHUB_BRANCH"
else
  echo "Allowing all branches"
fi
echo "IAM Role Name: $ROLE_NAME"
echo ""

# Set AWS region for CLI commands
export AWS_DEFAULT_REGION=$AWS_REGION

# Check if AWS CLI is installed and configured
echo "Checking AWS CLI configuration..."
if ! aws sts get-caller-identity &>/dev/null; then
  echo "Error: AWS CLI not configured properly. Please run 'aws configure' first."
  exit 1
fi

# Create OIDC provider if it doesn't exist
echo "Checking if GitHub OIDC provider already exists..."
if aws iam list-open-id-connect-providers | grep -q "token.actions.githubusercontent.com"; then
  echo "GitHub OIDC provider already exists."
else
  echo "Creating GitHub OIDC provider..."

  # Get GitHub OIDC thumbprint
  echo "Fetching GitHub OIDC thumbprint..."
  THUMBPRINT=$(openssl s_client -servername token.actions.githubusercontent.com -showcerts -connect token.actions.githubusercontent.com:443 </dev/null 2>/dev/null |
               openssl x509 -in /dev/stdin -noout -fingerprint -sha1 |
               sed 's/://g' | sed 's/.*=//')

  if [[ -z $THUMBPRINT ]]; then
    echo "Error: Failed to get GitHub OIDC thumbprint."
    exit 1
  fi

  # Create OIDC provider
  aws iam create-open-id-connect-provider \
    --url "https://token.actions.githubusercontent.com" \
    --client-id-list "sts.amazonaws.com" \
    --thumbprint-list "$THUMBPRINT" \
    --tags Key=ManagedBy,Value=setup-github-oidc.sh

  echo "GitHub OIDC provider created successfully."
fi

# Get provider ARN
PROVIDER_ARN="arn:aws:iam::$AWS_ACCOUNT_ID:oidc-provider/token.actions.githubusercontent.com"

# Create trust policy
echo "Creating trust policy..."

if [[ $RESTRICT_BRANCH == true ]]; then
  SUB_VALUE="repo:$GITHUB_ORG/$GITHUB_REPO:ref:refs/heads/$GITHUB_BRANCH"
else
  SUB_VALUE="repo:$GITHUB_ORG/$GITHUB_REPO:*"
fi

TRUST_POLICY=$(cat <<EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "$PROVIDER_ARN"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "token.actions.githubusercontent.com:aud": "sts.amazonaws.com"
        },
        "StringLike": {
          "token.actions.githubusercontent.com:sub": "$SUB_VALUE"
        }
      }
    }
  ]
}
EOF
)

# Save trust policy to a temporary file
TRUST_POLICY_FILE=$(mktemp)
echo "$TRUST_POLICY" > "$TRUST_POLICY_FILE"

# Check if role exists
if aws iam get-role --role-name "$ROLE_NAME" &>/dev/null; then
  echo "Role $ROLE_NAME already exists. Updating trust policy..."
  aws iam update-assume-role-policy \
    --role-name "$ROLE_NAME" \
    --policy-document "file://$TRUST_POLICY_FILE"
else
  echo "Creating role $ROLE_NAME..."

  aws iam create-role \
    --role-name "$ROLE_NAME" \
    --assume-role-policy-document "file://$TRUST_POLICY_FILE" \
    --description "Role for GitHub Actions ($GITHUB_ORG/$GITHUB_REPO)" \
    --tags Key=ManagedBy,Value=setup-github-oidc.sh Key=Repository,Value="$GITHUB_ORG/$GITHUB_REPO"

  # Attach needed permissions policy (example for S3 access)
  echo "Creating S3 access policy..."
  
  POLICY_NAME="${ROLE_NAME}-s3-policy"
  POLICY_DOC=$(cat <<EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:ListBucket"
      ],
      "Resource": "arn:aws:s3:::*"
    },
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject",
        "s3:DeleteObject"
      ],
      "Resource": "arn:aws:s3:::*/*"
    }
  ]
}
EOF
)

  # Save policy to a temporary file
  POLICY_FILE=$(mktemp)
  echo "$POLICY_DOC" > "$POLICY_FILE"
  
  # Create and attach policy
  POLICY_ARN=$(aws iam create-policy \
    --policy-name "$POLICY_NAME" \
    --policy-document "file://$POLICY_FILE" \
    --query 'Policy.Arn' --output text)
  
  aws iam attach-role-policy \
    --role-name "$ROLE_NAME" \
    --policy-arn "$POLICY_ARN"
    
  echo "Policy $POLICY_NAME created and attached to role $ROLE_NAME."
  
  # Clean up temporary file
  rm "$POLICY_FILE"
fi

# Clean up temporary file
rm "$TRUST_POLICY_FILE"

# Output role ARN
ROLE_ARN="arn:aws:iam::$AWS_ACCOUNT_ID:role/$ROLE_NAME"
echo ""
echo "✅ Setup completed successfully!"
echo ""
echo "Role ARN: $ROLE_ARN"
echo ""
echo "Add the following secrets to your GitHub repository:"
echo "AWS_ROLE_ARN: $ROLE_ARN"
echo "AWS_REGION: $AWS_REGION"
echo "AWS_ACCOUNT_ID: $AWS_ACCOUNT_ID"
echo ""
echo "Make sure your GitHub Actions workflow has the following permissions:"
echo "permissions:"
echo "  id-token: write"
echo "  contents: read"