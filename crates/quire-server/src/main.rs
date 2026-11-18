#![forbid(unsafe_code)]

use anyhow::Context;
use quire_server::{AppState, Config, build_router};
use quire_store::PgReportStore;
use secrecy::ExposeSecret;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,quire_server=debug,quire_store=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = Config::from_env().context("invalid server configuration")?;
    let store = PgReportStore::connect(
        config.database_url.expose_secret(),
        config.redis_url.expose_secret(),
    )
    .await
    .context("failed to connect to PostgreSQL/Redis")?;
    if config.auto_migrate {
        store.migrate().await.context("failed to run migrations")?;
    }
    store
        .health()
        .await
        .context("dependency health check failed")?;

    let bind_addr = config.bind_addr;
    let state = AppState::new(store, config);
    let router = build_router(state);

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;
    info!(
        %bind_addr,
        "Quire report engine listening (REST /api/*, SSE /api/docs/{{id}}/events, MCP /mcp)"
    );

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    warn!("shutdown signal received; draining connections");
}
