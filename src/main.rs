use axum::{Router, extract::State, response::Html, routing::get};
use minijinja::{Environment, path_loader};
use std::sync::Arc;

// Define a struct to hold our application state
struct AppState {
    templates: Environment<'static>,
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

#[tokio::main]
async fn main() {
    // Set up the template environment
    let mut env = Environment::new();
    env.set_loader(path_loader("templates"));

    // Create the application state
    let state = Arc::new(AppState { templates: env });

    // Set up the routes
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/about", get(about_handler))
        .with_state(state);

    println!("Server starting on http://0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
