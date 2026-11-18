//! `quire-runtime` binary entrypoint.

#![forbid(unsafe_code)]

use std::sync::Arc;

use anyhow::Context;
use quire_runtime::{
    Config, HttpQuireControl, KbService, ModelRegistry, RuntimeService, RuntimeStore, http,
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
    let models = ModelRegistry::new(
        store.clone(),
        config.master_key.clone(),
        &config.models_dev_url,
    );
    let quire = HttpQuireControl::new(&config.quire_base_url, config.quire_rw_token.clone());
    let service = RuntimeService::new(
        store,
        kb,
        Arc::new(models),
        Arc::new(config.clone()),
        Arc::new(quire),
    );
    let recovered = service.boot().await.context("runtime boot recovery")?;
    let seed = service.seed().await.context("runtime knowledge seed")?;
    info!(
        recovered_runs = recovered,
        seeded_documents = seed.documents,
        seeded_agents = seed.agents,
        seeded_skills = seed.skills,
        kb_version = seed.kb_version,
        bind_addr = %config.bind_addr,
        "quire-runtime ready"
    );
    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .context("bind runtime HTTP listener")?;
    axum::serve(listener, http::router(service))
        .await
        .context("serve runtime HTTP")?;
    Ok(())
}
