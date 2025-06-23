use crate::{
    auth::{
        session::{SessionData, SessionStore, create_logout_cookie, create_session_cookie},
        spotify_client::SpotifyClientWrapper,
    },
    config::Config,
    db::{DbPool, create_user, get_user_by_spotify_username},
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::cookie::CookieJar;
use rspotify::{AuthCodePkceSpotify, clients::OAuthClient, model::PrivateUser, prelude::Id};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info};

/// State shared between handlers
#[derive(Clone)]
pub struct AuthState {
    pub spotify_client: SpotifyClientWrapper,
    pub session_store: SessionStore,
    pub oauth_flow_store: Arc<Mutex<HashMap<String, AuthCodePkceSpotify>>>,
    pub db_pool: DbPool,
    pub config: Config,
}

/// OAuth callback query parameters
#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// User info response
#[derive(Serialize)]
pub struct UserInfo {
    id: i64,
    spotify_username: String,
    authenticated: bool,
}

/// Handler to initiate OAuth login flow
#[axum::debug_handler]
pub async fn auth_login(State(state): State<Arc<AuthState>>) -> impl IntoResponse {
    // Create a new client instance for this OAuth flow
    let (url, client) = match state.spotify_client.create_oauth_flow() {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to generate authorization URL: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Failed to start login process. Please try again."),
            )
                .into_response();
        }
    };

    // Extract state parameter from URL
    let state_param = match url.split("state=").nth(1) {
        Some(part) => match part.split('&').next() {
            Some(state) => state.to_string(),
            None => {
                error!("Failed to extract state from URL");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html("Failed to start login process. Please try again."),
                )
                    .into_response();
            }
        },
        None => {
            error!("No state parameter in authorization URL");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Failed to start login process. Please try again."),
            )
                .into_response();
        }
    };

    // Store the client using state as key
    let mut flow_store = state.oauth_flow_store.lock().await;
    flow_store.insert(state_param, client);

    info!("Redirecting to Spotify authorization URL");
    Redirect::to(&url).into_response()
}

/// Handler for OAuth callback
#[axum::debug_handler]
pub async fn auth_callback(
    State(state): State<Arc<AuthState>>,
    Query(query): Query<CallbackQuery>,
    cookies: CookieJar,
) -> impl IntoResponse {
    // Check for OAuth errors
    if let Some(error) = query.error {
        error!("OAuth error: {}", error);
        return (
            StatusCode::BAD_REQUEST,
            Html(format!("Authentication failed: {}", error)),
        )
            .into_response();
    }

    // Get authorization code
    let code = match query.code {
        Some(code) => code,
        None => {
            error!("No authorization code in callback");
            return (
                StatusCode::BAD_REQUEST,
                Html("No authorization code received"),
            )
                .into_response();
        }
    };

    // Get state parameter from callback
    let state_param = match query.state {
        Some(state) => state,
        None => {
            error!("No state parameter in callback");
            return (
                StatusCode::BAD_REQUEST,
                Html("Invalid authentication state"),
            )
                .into_response();
        }
    };

    // Retrieve the client from the flow store
    let mut client = {
        let mut flow_store = state.oauth_flow_store.lock().await;
        match flow_store.remove(&state_param) {
            Some(client) => client,
            None => {
                error!("No OAuth flow found for state: {}", state_param);
                return (
                    StatusCode::BAD_REQUEST,
                    Html("Invalid or expired authentication session"),
                )
                    .into_response();
            }
        }
    };

    // Exchange code for tokens using the same client instance
    if let Err(e) = client.request_token(&code).await {
        error!("Failed to exchange code for token: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html("Failed to complete authentication"),
        )
            .into_response();
    }

    // Get the token from the client
    let token = {
        let token_lock = client.token.lock().await.unwrap();
        match token_lock.as_ref() {
            Some(token) => token.clone(),
            None => {
                error!("No token received after exchange");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html("Failed to receive authentication token"),
                )
                    .into_response();
            }
        }
    };

    // Get user profile using the authenticated client
    let spotify_user: PrivateUser = match client.current_user().await {
        Ok(user) => user,
        Err(e) => {
            error!("Failed to get user profile: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Failed to get user information"),
            )
                .into_response();
        }
    };

    let spotify_username = spotify_user.id.id().to_string();

    // Create or get user from database
    let user = match get_user_by_spotify_username(&state.db_pool, &spotify_username).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // Create new user
            match create_user(&state.db_pool, &spotify_username).await {
                Ok(user) => {
                    info!("Created new user: {}", spotify_username);
                    user
                }
                Err(e) => {
                    error!("Failed to create user: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Html("Failed to create user account"),
                    )
                        .into_response();
                }
            }
        }
        Err(e) => {
            error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Database error occurred"),
            )
                .into_response();
        }
    };

    // Store tokens in database
    if let Err(e) = state
        .spotify_client
        .store_tokens_for_user(&state.db_pool, user.id, &token)
        .await
    {
        error!("Failed to store tokens: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html("Failed to save authentication tokens"),
        )
            .into_response();
    }

    // Create session
    let session_data = SessionData {
        user_id: user.id,
        spotify_username: user.spotify_username,
        created_at: chrono::Utc::now(),
    };

    let session_id = state.session_store.create_session(session_data);

    // Set session cookie
    let secure =
        !state.config.host.starts_with("localhost") && !state.config.host.starts_with("127.0.0.1");
    let updated_cookies = cookies.add(create_session_cookie(session_id, secure));

    info!("User {} successfully authenticated", spotify_username);

    // Redirect to home page with updated cookies
    (updated_cookies, Redirect::to("/")).into_response()
}

/// Handler to logout user
#[axum::debug_handler]
pub async fn auth_logout(
    State(state): State<Arc<AuthState>>,
    cookies: CookieJar,
) -> impl IntoResponse {
    // Get session cookie and remove from store
    if let Some(session_cookie) = cookies.get("session_id") {
        let session_id = session_cookie.value();
        state.session_store.remove_session(session_id);
    }

    // Clear session cookie
    let updated_cookies = cookies.add(create_logout_cookie());

    info!("User logged out");
    (updated_cookies, Redirect::to("/"))
}

/// Handler to get current user info
#[axum::debug_handler]
pub async fn auth_me(State(state): State<Arc<AuthState>>, cookies: CookieJar) -> impl IntoResponse {
    // Get session cookie
    if let Some(session_cookie) = cookies.get("session_id") {
        let session_id = session_cookie.value();

        // Get session data from store
        if let Some(session_data) = state.session_store.get_session(session_id) {
            return Json(UserInfo {
                id: session_data.user_id,
                spotify_username: session_data.spotify_username,
                authenticated: true,
            });
        }
    }

    Json(UserInfo {
        id: 0,
        spotify_username: String::new(),
        authenticated: false,
    })
}

/// Error handler for auth routes
pub async fn auth_error_handler() -> impl IntoResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html("An authentication error occurred"),
    )
}
