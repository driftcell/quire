//! Standalone migration runner that does not compile application query macros.

#![forbid(unsafe_code)]

use anyhow::Context;
use sqlx::PgPool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let pool = PgPool::connect(&database_url)
        .await
        .context("connect migration PostgreSQL database")?;

    let mut report = sqlx::migrate!("../quire-store/migrations");
    report.create_schema("report");
    report.dangerous_set_table_name("report._sqlx_migrations");
    report.run(&pool).await.context("run report migrations")?;

    let mut runtime = sqlx::migrate!("../quire-runtime/migrations");
    runtime.create_schema("runtime");
    runtime.dangerous_set_table_name("runtime._quire_runtime_migrations");
    runtime.run(&pool).await.context("run runtime migrations")?;

    pool.close().await;
    Ok(())
}
