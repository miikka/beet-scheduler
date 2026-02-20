use axum::{
    routing::{get, post},
    Router,
};
use minijinja::Environment;
use std::sync::Arc;
use tower_http::services::ServeDir;

pub mod db;
pub mod error;
pub mod handlers;
pub mod models;

use db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub env: Arc<Environment<'static>>,
}

impl axum::extract::FromRef<AppState> for Db {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<Environment<'static>> {
    fn from_ref(state: &AppState) -> Self {
        state.env.clone()
    }
}

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::home::show))
        .route("/slots/new-row", get(handlers::home::new_slot_row))
        .route("/meetings", post(handlers::meetings::create))
        .route("/m/{id}", get(handlers::respond::show_meeting))
        .route("/m/{id}/responses", post(handlers::respond::submit_response))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}
