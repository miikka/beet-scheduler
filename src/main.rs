// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use beet_scheduler::{AppState, add_globals, build_app, rate_limit::RateLimiter};
use minijinja::Environment;
use std::{net::SocketAddr, sync::Arc};
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
    beet_scheduler::db::set_max_size(&db, app_config.max_db_size_mb * 1024 * 1024)?;
    if app_config.retention_days > 0 {
        tokio::spawn(beet_scheduler::db::expire_periodically(
            db.clone(),
            app_config.retention_days,
        ));
    }

    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    add_globals(&mut env, app_config.html_snippet);

    let state = AppState {
        db,
        env: Arc::new(env),
        rate_limiter: Arc::new(RateLimiter::new(
            app_config.write_rate_limit_per_minute,
            app_config.trust_forwarded_for,
        )),
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
