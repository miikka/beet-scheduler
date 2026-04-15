// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use axum::{
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use minijinja::Environment;
use std::sync::Arc;
use tower_http::services::ServeDir;

pub mod config;
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

/// Register app-level globals on the minijinja environment.
/// Call this before wrapping `env` in `Arc`.
pub fn add_globals(env: &mut Environment<'static>, html_snippet: String) {
    env.add_global("html_snippet", html_snippet);
}

async fn robots_txt() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain")],
        "User-agent: *\nDisallow: /\n",
    )
}

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/robots.txt", get(robots_txt))
        .route("/", get(handlers::home::show))
        .route("/slots/new-row", get(handlers::home::new_slot_row))
        .route("/meetings", post(handlers::meetings::create))
        .route("/m/{id}", get(handlers::respond::show_meeting))
        .route(
            "/m/{id}/responses",
            post(handlers::respond::submit_response),
        )
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}
