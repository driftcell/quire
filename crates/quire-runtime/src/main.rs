//! Unified Quire `server` binary entrypoint.

#![forbid(unsafe_code)]

use std::sync::Arc;

use anyhow::Context;
use quire_runtime::{
    Config, KbService, ModelRegistry, RigModelStreamProvider, RuntimeService, RuntimeStore, http,
    sandbox::build_sandbox_provider,
};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,quire_runtime=debug".into()),
        )
        .init();
    let config = Config::from_env();
    let store = RuntimeStore::connect(&config.database_url, &config.redis_url)
        .await
        .context("connect runtime PostgreSQL/Redis")?;
    let auto_migrate = std::env::var("RUNTIME_AUTO_MIGRATE")
        .map(|value| value != "false")
        .unwrap_or(true);
    if auto_migrate {
        store.migrate().await.context("run runtime migrations")?;
    }
    let kb = KbService::new(store.clone());
    let models = Arc::new(ModelRegistry::new(
        store.clone(),
        config.master_key.clone(),
        &config.models_dev_url,
    ));
    if let Err(error) = models.resolve(&config.default_model).await {
        tracing::warn!(
            %error,
            model = %config.default_model,
            "default model route is not active; register a provider/model before starting sessions"
        );
    }
    let sandbox_provider =
        build_sandbox_provider(&config.sandbox).context("configure sandbox provider")?;
    let runtime_config = Arc::new(config.clone());
    let service = RuntimeService::new(store, kb.clone(), models.clone(), runtime_config.clone());
    let mcp = service.mcp_registry();
    let queue = service.pending_queue();
    let service = service.with_model_stream_provider(Arc::new(RigModelStreamProvider::new(
        kb,
        models,
        runtime_config,
        mcp,
        sandbox_provider,
        queue,
    )));
    let recovered = service.boot().await.context("runtime boot recovery")?;
    let seed = service.seed().await.context("runtime knowledge seed")?;
    info!(
        recovered_runs = recovered,
        seeded_documents = seed.documents,
        seeded_agents = seed.agents,
        seeded_skills = seed.skills,
        kb_version = seed.kb_version,
        bind_addr = %config.bind_addr,
        "server ready"
    );
    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .context("bind runtime HTTP listener")?;
    axum::serve(listener, http::router(service))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve runtime HTTP")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sigterm) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sigterm.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
