use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use time::Duration;
use uuid::Uuid;

/// Session data stored for authenticated users
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub user_id: i64,
    pub spotify_username: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// In-memory session store
#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
}

impl SessionStore {
    /// Create a new session store
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session
    pub fn create_session(&self, data: SessionData) -> String {
        let session_id = Uuid::new_v4().to_string();
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(session_id.clone(), data);
        session_id
    }

    /// Get session data by ID
    pub fn get_session(&self, session_id: &str) -> Option<SessionData> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(session_id).cloned()
    }

    /// Remove a session
    pub fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(session_id);
    }

    /// Clean up expired sessions (for future implementation)
    pub fn cleanup_expired(&self) {
        // For now, sessions don't expire in memory
        // In production, you'd want to implement expiration
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait to make session data available in requests
#[derive(Clone)]
pub struct CurrentUser(pub SessionData);

/// Middleware to check authentication and add user to request extensions
pub async fn session_middleware(
    State(store): State<SessionStore>,
    cookies: CookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    // Get session cookie
    if let Some(session_cookie) = cookies.get("session_id") {
        let session_id = session_cookie.value();

        // Get session data from store
        if let Some(session_data) = store.get_session(session_id) {
            // Add user data to request extensions
            request.extensions_mut().insert(CurrentUser(session_data));
        }
    }

    next.run(request).await
}

/// Middleware that requires authentication
pub async fn require_auth(
    cookies: CookieJar,
    State(store): State<SessionStore>,
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    // Get session cookie
    if let Some(session_cookie) = cookies.get("session_id") {
        let session_id = session_cookie.value();

        // Check if session exists
        if store.get_session(session_id).is_some() {
            return Ok(next.run(request).await);
        }
    }

    // Redirect to login if not authenticated
    Err(Redirect::to("/auth/login").into_response())
}

/// Helper function to create session cookie
pub fn create_session_cookie(session_id: String, secure: bool) -> Cookie<'static> {
    Cookie::build(("session_id", session_id))
        .path("/")
        .secure(secure)
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .permanent()
        .build()
}

/// Helper function to create logout cookie
pub fn create_logout_cookie() -> Cookie<'static> {
    Cookie::build(("session_id", ""))
        .path("/")
        .max_age(Duration::seconds(0))
        .build()
}
