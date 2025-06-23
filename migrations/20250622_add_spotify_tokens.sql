-- Add Spotify OAuth token fields to users table
ALTER TABLE users ADD COLUMN access_token TEXT;
ALTER TABLE users ADD COLUMN refresh_token TEXT;
ALTER TABLE users ADD COLUMN token_expires_at TIMESTAMP;
ALTER TABLE users ADD COLUMN token_scopes TEXT;

-- Create index on token expiration for efficient queries
CREATE INDEX idx_users_token_expires_at ON users (token_expires_at);
