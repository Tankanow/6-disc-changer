use axum::{
    Router,
    extract::{Form, State},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use dotenv::dotenv;
use minijinja::{Environment, path_loader};
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod config;
mod database;
mod db;
mod shutdown;

use auth::{
    AuthState, SessionStore, SpotifyClientWrapper, auth_callback, auth_login, auth_logout, auth_me,
};
use config::Config;
use database::{
    BackupManager, BackupScheduler, RestorationChecker, backup::BackupOptions,
    create_shared_restoration_status, create_shared_status, storage::create_storage_provider,
};
use shutdown::{ShutdownManager, setup_signal_handlers};
use tokio::time::Duration;

// Define a struct to hold our application state
struct AppState {
    templates: Environment<'static>,
    db_pool: db::DbPool,
    backup_manager: Option<Arc<BackupManager>>,
    backup_scheduler: Option<Arc<BackupScheduler>>,
    session_store: SessionStore,
}

// Handler for the index route
async fn index_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let template = state.templates.get_template("index.html").unwrap();
    let rendered = template.render(minijinja::context! {}).unwrap();
    Html(rendered)
}

// Handler for the about route
async fn about_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let template = state.templates.get_template("about.html").unwrap();
    let rendered = template.render(minijinja::context! {}).unwrap();
    Html(rendered)
}

// Handler for the users page
async fn users_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let template = state.templates.get_template("users.html").unwrap();
    let rendered = template.render(minijinja::context! {}).unwrap();
    Html(rendered)
}

// Handler to list all users (for HTMX)
async fn list_users_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    // Get all users from the database
    let users = db::get_all_users(&state.db_pool).await.unwrap_or_default();

    // Render just the user list portion
    let template = state.templates.get_template("user_list.html").unwrap();
    let rendered = template
        .render(minijinja::context! {
            users => users
        })
        .unwrap();

    Html(rendered)
}

// Form data for adding a user
#[derive(Deserialize)]
struct AddUserForm {
    spotify_username: String,
}

// Handler to add a new user
async fn add_user_handler(
    State(state): State<Arc<AppState>>,
    Form(form): Form<AddUserForm>,
) -> impl IntoResponse {
    // Add user to the database
    match db::create_user(&state.db_pool, &form.spotify_username).await {
        Ok(user) => {
            // Render the individual user item for HTMX to append
            let template = state.templates.get_template("user_list_item.html").unwrap();
            let rendered = template
                .render(minijinja::context! {
                    user => user
                })
                .unwrap();

            Html(rendered)
        }
        Err(_) => {
            // Return an error message
            Html(String::from("Failed to add user"))
        }
    }
}

#[tokio::main]
async fn main() {
    // Load .env file
    dotenv().ok();

    // Initialize tracing subscriber for structured logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Set up the template environment
    let mut env = Environment::new();
    env.set_loader(path_loader("templates"));

    // Load configuration
    let config = Config::from_env();

    // Check if database restoration is needed before initialization
    let restoration_performed = if config.backup.force_restoration
        || !config.backup.database_path.exists()
    {
        match create_storage_provider(&config.backup).await {
            Ok(storage) => {
                let restoration_status = create_shared_restoration_status();
                let restoration_checker = RestorationChecker::new(
                    config.backup.database_path.clone(),
                    storage.into(),
                    &config.backup.environment,
                    config.backup.server_id.as_deref(),
                    restoration_status,
                );

                match restoration_checker.check_and_restore_if_needed().await {
                    Ok(restored) => {
                        if restored {
                            info!("Database restored from backup");
                        }
                        restored
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to restore database: {}. Starting with fresh database.",
                            e
                        );
                        false
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize storage for restoration: {}. Starting with fresh database.",
                    e
                );
                false
            }
        }
    } else {
        info!("Database exists and force restoration not requested, skipping restoration check");
        false
    };

    // Initialize the database
    let db_pool = db::init_db(&config.backup.database_path)
        .await
        .expect("Failed to initialize database");
    info!(
        "Database initialized successfully (restored: {})",
        restoration_performed
    );

    // Initialize backup infrastructure
    let backup_status = create_shared_status();
    let backup_manager = match create_storage_provider(&config.backup).await {
        Ok(storage) => {
            let manager = BackupManager::new(
                db_pool.clone(),
                storage.into(),
                &config.backup.environment,
                config.backup.server_id.as_deref(),
                backup_status.clone(),
            );
            info!("Backup manager initialized successfully");
            Some(Arc::new(manager))
        }
        Err(e) => {
            tracing::error!("Failed to initialize backup infrastructure: {}", e);
            None
        }
    };

    // Create shutdown manager if backup manager exists
    let shutdown_manager = backup_manager
        .as_ref()
        .map(|manager| Arc::new(ShutdownManager::new(manager.clone())));

    // Create backup scheduler if backup manager exists
    let backup_scheduler = if let Some(ref manager) = backup_manager {
        let scheduler = Arc::new(BackupScheduler::new(manager.clone(), backup_status.clone()));

        // Start the scheduler with configured interval
        let interval = Duration::from_secs(config.backup.backup_interval_seconds);
        let options = BackupOptions::default();

        match scheduler.start(interval, options).await {
            Ok(_) => {
                tracing::info!(
                    "Backup scheduler started with interval: {} seconds",
                    config.backup.backup_interval_seconds
                );

                // Set scheduler reference in shutdown manager
                if let Some(ref shutdown_mgr) = shutdown_manager {
                    shutdown_mgr.set_scheduler(scheduler.clone()).await;
                }

                Some(scheduler)
            }
            Err(e) => {
                tracing::error!("Failed to start backup scheduler: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Initialize session store
    let session_store = SessionStore::new();

    // Initialize Spotify client if configured
    let spotify_client = if config.spotify.is_configured() {
        Some(SpotifyClientWrapper::new(
            config.spotify.client_id.clone(),
            config.spotify.redirect_uri.clone(),
        ))
    } else {
        tracing::warn!("Spotify credentials not configured, OAuth features will be disabled");
        None
    };

    // Create the application state
    let state = Arc::new(AppState {
        templates: env,
        db_pool: db_pool.clone(),
        backup_manager,
        backup_scheduler,
        session_store: session_store.clone(),
    });

    // Create auth state if Spotify is configured
    let auth_state = spotify_client.map(|client| {
        Arc::new(AuthState {
            spotify_client: client,
            session_store: session_store.clone(),
            oauth_flow_store: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            db_pool: db_pool.clone(),
            config: config.clone(),
        })
    });

    // Set up the main routes
    let mut app = Router::new()
        .route("/", get(index_handler))
        .route("/about", get(about_handler))
        .route("/users", get(users_handler))
        .route("/users", post(add_user_handler))
        .route("/users/list", get(list_users_handler))
        .with_state(state.clone());

    // Add auth routes if Spotify is configured
    if let Some(auth_state) = auth_state {
        let auth_routes = Router::new()
            .route("/auth/login", get(auth_login))
            .route("/auth/callback", get(auth_callback))
            .route("/auth/logout", get(auth_logout))
            .route("/auth/me", get(auth_me))
            .with_state(auth_state);

        app = app.merge(auth_routes);
    }

    // Add middleware
    // Cookie handling is now done via axum-extra

    // Set up signal handlers if we have a shutdown manager
    let shutdown_signal = if let Some(ref shutdown_mgr) = shutdown_manager {
        info!("Setting up signal handlers for graceful shutdown");
        setup_signal_handlers(shutdown_mgr.clone()).await;
        Some(shutdown_mgr.get_shutdown_signal())
    } else {
        None
    };

    info!("Server starting on http://0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    // Run the server with graceful shutdown if shutdown manager is available
    if let Some(signal) = shutdown_signal {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                signal.notified().await;
                info!("Graceful shutdown complete");
            })
            .await
            .unwrap();
    } else {
        axum::serve(listener, app).await.unwrap();
    }

    info!("Server shut down");
}
