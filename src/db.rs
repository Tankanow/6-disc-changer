use serde::{Deserialize, Serialize};
use sqlx::{
    Connection, Pool, Row, Sqlite,
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;
use std::str::FromStr;
use tracing::info;

// Database connection pool type
pub type DbPool = Pool<Sqlite>;

/// Initialize the database, running migrations if necessary
pub async fn init_db(database_path: &Path) -> Result<DbPool, sqlx::Error> {
    let db_url = format!("sqlite:{}", database_path.display());
    info!("Initializing database at {}", &db_url);

    // Create database if it doesn't exist
    if !Sqlite::database_exists(&db_url).await.unwrap_or(false) {
        info!("Database does not exist; creating");
        Sqlite::create_database(&db_url).await?;
    }

    info!("Setting up connection options");
    let options = SqliteConnectOptions::from_str(&db_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    info!("Creating connection pool");
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    info!("Running migrations");
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

/// Check database health and integrity
pub async fn check_database_health(database_path: &Path) -> Result<bool, sqlx::Error> {
    let db_url = format!("sqlite:{}", database_path.display());

    // Try to connect
    let mut conn = match sqlx::SqliteConnection::connect(&db_url).await {
        Ok(conn) => conn,
        Err(_) => return Ok(false),
    };

    // Run integrity check
    let result = sqlx::query("PRAGMA integrity_check")
        .fetch_one(&mut conn)
        .await;

    match result {
        Ok(row) => {
            let check_result: String = row.try_get(0).unwrap_or_else(|_| "error".to_string());
            Ok(check_result == "ok")
        }
        Err(_) => Ok(false),
    }
}

/// Get a user by Spotify username
pub async fn get_user_by_spotify_username(
    pool: &DbPool,
    spotify_username: &str,
) -> Result<Option<User>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, spotify_username, created_at, updated_at,
               access_token, refresh_token, token_expires_at, token_scopes
        FROM users
        WHERE spotify_username = ?
        "#,
    )
    .bind(spotify_username)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        Ok(Some(User {
            id: row.try_get("id")?,
            spotify_username: row.try_get("spotify_username")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            access_token: row.try_get("access_token")?,
            refresh_token: row.try_get("refresh_token")?,
            token_expires_at: row.try_get("token_expires_at")?,
            token_scopes: row.try_get("token_scopes")?,
        }))
    } else {
        Ok(None)
    }
}

/// Create a new user
pub async fn create_user(pool: &DbPool, spotify_username: &str) -> Result<User, sqlx::Error> {
    // Insert user
    sqlx::query(
        r#"
        INSERT INTO users (spotify_username)
        VALUES (?)
        "#,
    )
    .bind(spotify_username)
    .execute(pool)
    .await?;

    // Get created user
    match get_user_by_spotify_username(pool, spotify_username).await? {
        Some(user) => Ok(user),
        None => Err(sqlx::Error::RowNotFound),
    }
}

/// Get all users
pub async fn get_all_users(pool: &DbPool) -> Result<Vec<User>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, spotify_username, created_at, updated_at,
               access_token, refresh_token, token_expires_at, token_scopes
        FROM users
        ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut users = Vec::with_capacity(rows.len());
    for row in rows {
        users.push(User {
            id: row.try_get("id")?,
            spotify_username: row.try_get("spotify_username")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            access_token: row.try_get("access_token")?,
            refresh_token: row.try_get("refresh_token")?,
            token_expires_at: row.try_get("token_expires_at")?,
            token_scopes: row.try_get("token_scopes")?,
        });
    }

    Ok(users)
}

/// Update OAuth tokens for a user
pub async fn update_user_tokens(
    pool: &DbPool,
    user_id: i64,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: chrono::DateTime<chrono::Utc>,
    scopes: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE users
        SET access_token = ?,
            refresh_token = ?,
            token_expires_at = ?,
            token_scopes = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at)
    .bind(scopes)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get a user by ID
pub async fn get_user_by_id(pool: &DbPool, user_id: i64) -> Result<Option<User>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, spotify_username, created_at, updated_at,
               access_token, refresh_token, token_expires_at, token_scopes
        FROM users
        WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        Ok(Some(User {
            id: row.try_get("id")?,
            spotify_username: row.try_get("spotify_username")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            access_token: row.try_get("access_token")?,
            refresh_token: row.try_get("refresh_token")?,
            token_expires_at: row.try_get("token_expires_at")?,
            token_scopes: row.try_get("token_scopes")?,
        }))
    } else {
        Ok(None)
    }
}

// User model
#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub spotify_username: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub token_scopes: Option<String>,
}

// Implement FromRow for User to allow for conversion from database rows
impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for User {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(User {
            id: row.try_get("id")?,
            spotify_username: row.try_get("spotify_username")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            access_token: row.try_get("access_token")?,
            refresh_token: row.try_get("refresh_token")?,
            token_expires_at: row.try_get("token_expires_at")?,
            token_scopes: row.try_get("token_scopes")?,
        })
    }
}
