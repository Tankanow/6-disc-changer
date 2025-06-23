use crate::db::{DbPool, get_user_by_id, update_user_tokens};
use chrono::{Duration, Utc};
use rspotify::{
    AuthCodePkceSpotify, ClientError, Config as RSpotifyConfig, Credentials, OAuth, Token,
    clients::{BaseClient, OAuthClient},
    scopes,
};
use std::sync::Arc;

/// Wrapper around rspotify's AuthCodePkceSpotify client
#[derive(Clone)]
pub struct SpotifyClientWrapper {
    client: Arc<AuthCodePkceSpotify>,
    redirect_uri: String,
}

impl SpotifyClientWrapper {
    /// Create a new Spotify client wrapper with PKCE flow
    pub fn new(client_id: String, redirect_uri: String) -> Self {
        // Create credentials for PKCE (no client_secret needed)
        let creds = Credentials::new_pkce(&client_id);

        // Create OAuth object with required scopes
        let oauth = OAuth {
            redirect_uri: redirect_uri.clone(),
            scopes: scopes!(
                "user-read-private",
                "user-read-email",
                "streaming",
                "user-modify-playback-state",
                "user-read-playback-state"
            ),
            ..Default::default()
        };

        // Configure the client
        let config = RSpotifyConfig {
            ..Default::default()
        };

        // Create the PKCE client
        let client = AuthCodePkceSpotify::with_config(creds, oauth, config);

        Self {
            client: Arc::new(client),
            redirect_uri,
        }
    }

    /// Generate the authorization URL for the OAuth flow
    pub fn get_authorize_url(&self) -> Result<String, ClientError> {
        let mut client = (*self.client).clone();
        client.get_authorize_url(None)
    }

    /// Create a new OAuth flow with a fresh client instance
    pub fn create_oauth_flow(&self) -> Result<(String, AuthCodePkceSpotify), ClientError> {
        // Create a new client instance for this OAuth flow
        let creds = Credentials::new_pkce(&self.client.creds.id);

        let oauth = OAuth {
            redirect_uri: self.redirect_uri.clone(),
            scopes: scopes!(
                "user-read-private",
                "user-read-email",
                "streaming",
                "user-modify-playback-state",
                "user-read-playback-state"
            ),
            ..Default::default()
        };

        let config = RSpotifyConfig {
            ..Default::default()
        };

        let mut client = AuthCodePkceSpotify::with_config(creds, oauth, config);

        // Generate the authorization URL (this also creates the code verifier)
        let url = client.get_authorize_url(None)?;

        Ok((url, client))
    }

    /// Exchange authorization code for access and refresh tokens
    pub async fn exchange_code(&self, code: &str) -> Result<Token, ClientError> {
        let mut client = (*self.client).clone();
        client.request_token(code).await?;

        // Get the token from the client after exchange
        let token_lock = client.token.lock().await.unwrap();
        let token = token_lock
            .as_ref()
            .ok_or_else(|| ClientError::CacheFile("No token received after exchange".to_string()))?
            .clone();
        drop(token_lock);

        Ok(token)
    }

    /// Store tokens in the database for a user
    pub async fn store_tokens_for_user(
        &self,
        pool: &DbPool,
        user_id: i64,
        token: &Token,
    ) -> Result<(), sqlx::Error> {
        let expires_at = token
            .expires_at
            .unwrap_or_else(|| Utc::now() + Duration::hours(1));

        let scopes = token
            .scopes
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        update_user_tokens(
            pool,
            user_id,
            &token.access_token,
            token.refresh_token.as_deref(),
            expires_at,
            &scopes,
        )
        .await
    }

    /// Create a client with existing tokens for a user
    pub async fn with_user_tokens(
        client_id: String,
        redirect_uri: String,
        pool: &DbPool,
        user_id: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        let user = get_user_by_id(pool, user_id).await?;

        if let Some(user) = user {
            if let (Some(access_token), Some(refresh_token)) =
                (user.access_token, user.refresh_token)
            {
                let wrapper = Self::new(client_id, redirect_uri);

                // Create token from stored data
                let token = Token {
                    access_token,
                    refresh_token: Some(refresh_token),
                    expires_in: Duration::seconds(0),
                    expires_at: user.token_expires_at,
                    scopes: user
                        .token_scopes
                        .map(|s| {
                            s.split_whitespace()
                                .map(|scope| scope.to_string())
                                .collect()
                        })
                        .unwrap_or_else(|| std::collections::HashSet::new()),
                };

                // Set the token on a mutable clone
                let mut client = (*wrapper.client).clone();
                *client.token.lock().await.unwrap() = Some(token);

                Ok(Some(wrapper))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Refresh the access token if expired
    pub async fn refresh_token_if_needed(
        &self,
        pool: &DbPool,
        user_id: i64,
    ) -> Result<bool, ClientError> {
        let mut client = (*self.client).clone();

        // Check if token needs refresh
        let needs_refresh = {
            let token_lock = client.token.lock().await.unwrap();
            if let Some(token) = &*token_lock {
                if let Some(expires_at) = token.expires_at {
                    expires_at <= Utc::now()
                } else {
                    false
                }
            } else {
                false
            }
        };

        if needs_refresh {
            // Refresh the token
            client.refresh_token().await?;

            // Get the refreshed token from the client
            let token_lock = client.token.lock().await.unwrap();
            if let Some(token) = &*token_lock {
                let token_clone = token.clone();
                drop(token_lock);

                self.store_tokens_for_user(pool, user_id, &token_clone)
                    .await
                    .map_err(|e| {
                        ClientError::CacheFile(format!("Failed to store tokens: {}", e))
                    })?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get the underlying rspotify client
    pub fn client(&self) -> &AuthCodePkceSpotify {
        &self.client
    }
}
