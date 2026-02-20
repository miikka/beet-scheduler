use beet_scheduler::{build_app, AppState};
use minijinja::Environment;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "beet_scheduler=debug,tower_http=debug".parse().unwrap()
        }))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db = beet_scheduler::db::open("beet-scheduler.db")?;

    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));

    let state = AppState {
        db,
        env: Arc::new(env),
    };

    let app = build_app(state).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("Listening on http://localhost:3000");
    axum::serve(listener, app).await?;

    Ok(())
}
