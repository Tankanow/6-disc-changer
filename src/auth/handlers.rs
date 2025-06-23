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
use rspotify::{clients::OAuthClient, model::PrivateUser, prelude::Id};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

/// State shared between handlers
#[derive(Clone)]
pub struct AuthState {
    pub spotify_client: SpotifyClientWrapper,
    pub session_store: SessionStore,
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
    match state.spotify_client.get_authorize_url() {
        Ok(url) => {
            info!("Redirecting to Spotify authorization URL");
            Redirect::to(&url).into_response()
        }
        Err(e) => {
            error!("Failed to generate authorization URL: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Failed to start login process. Please try again."),
            )
                .into_response()
        }
    }
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

    // Exchange code for tokens
    let token = match state.spotify_client.exchange_code(&code).await {
        Ok(token) => token,
        Err(e) => {
            error!("Failed to exchange code for token: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Failed to complete authentication"),
            )
                .into_response();
        }
    };

    // Create a new client with the token to get user info
    let mut client_with_token = (*state.spotify_client.client()).clone();
    *client_with_token.token.lock().await.unwrap() = Some(token.clone());

    // Get user profile
    let spotify_user: PrivateUser = match client_with_token.current_user().await {
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
