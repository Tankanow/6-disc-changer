#!/bin/bash

# Fix BackupConfig initializations in backup.rs tests

# Fix test_background_backup
perl -i -pe 'BEGIN{undef $/;} s/let config = BackupConfig \{\s*use_aws: false,\s*s3_bucket_name: String::new\(\),\s*aws_region: String::from\("us-west-2"\),\s*aws_role_arn: None,\s*local_backup_dir: backup_dir\.path\(\)\.to_path_buf\(\),\s*local_backup_max_count: 5,\s*environment: String::from\("test"\),\s*server_id: None,\s*\};/let config = BackupConfig {\n            local_backup_dir: backup_dir.path().to_path_buf(),\n            environment: String::from("test"),\n            ..Default::default()\n        };/smg' src/database/backup.rs

# Fix test_multiple_concurrent_backups
perl -i -pe 'BEGIN{undef $/;} s/let config = BackupConfig \{\s*use_aws: false,\s*s3_bucket_name: String::new\(\),\s*aws_region: String::from\("us-west-2"\),\s*aws_role_arn: None,\s*local_backup_dir: backup_dir\.path\(\)\.to_path_buf\(\),\s*local_backup_max_count: 5,\s*environment: String::from\("test"\),\s*server_id: Some\(format!\("server\{\}", i\)\),\s*\};/let config = BackupConfig {\n            local_backup_dir: backup_dir.path().to_path_buf(),\n            environment: String::from("test"),\n            server_id: Some(format!("server{}", i)),\n            ..Default::default()\n        };/smg' src/database/backup.rs

# Fix test_cleanup_completed_jobs
perl -i -pe 'BEGIN{undef $/;} s/let config = BackupConfig \{\s*use_aws: false,\s*s3_bucket_name: String::new\(\),\s*aws_region: String::from\("us-west-2"\),\s*aws_role_arn: None,\s*local_backup_dir: backup_dir\.path\(\)\.to_path_buf\(\),\s*local_backup_max_count: 5,\s*environment: String::from\("test"\),\s*server_id: None,\s*\};/let config = BackupConfig {\n            local_backup_dir: backup_dir.path().to_path_buf(),\n            environment: String::from("test"),\n            ..Default::default()\n        };/smg' src/database/backup.rs

echo "Fixed BackupConfig initializations in test files"
