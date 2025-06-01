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

mod db;
mod database;
mod config;

// Define a struct to hold our application state
struct AppState {
    templates: Environment<'static>,
    db_pool: db::DbPool,
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

    // Set up the template environment
    let mut env = Environment::new();
    env.set_loader(path_loader("templates"));

    // Initialize the database
    let db_pool = db::init_db().await.expect("Failed to initialize database");
    println!("Database initialized successfully");

    // Create the application state
    let state = Arc::new(AppState {
        templates: env,
        db_pool,
    });

    // Set up the routes
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/about", get(about_handler))
        .route("/users", get(users_handler))
        .route("/users", post(add_user_handler))
        .route("/users/list", get(list_users_handler))
        .with_state(state);

    println!("Server starting on http://0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
