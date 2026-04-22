// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use beet_scheduler::{AppState, add_globals, build_app};
use minijinja::Environment;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "beet_scheduler=debug,tower_http=debug".parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app_config = beet_scheduler::config::AppConfig::load()?;

    let db_path =
        std::env::var("DATABASE_PATH").unwrap_or_else(|_| "beet-scheduler.db".to_string());
    let db = beet_scheduler::db::open(&db_path)?;

    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    add_globals(&mut env, app_config.html_snippet);

    let state = AppState {
        db,
        env: Arc::new(env),
    };

    let app = build_app(state).layer(TraceLayer::new_for_http());

    let port: u16 = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("3000")
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid port number"))?;

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("Listening on http://localhost:{port}");
    axum::serve(listener, app).await?;

    Ok(())
}
