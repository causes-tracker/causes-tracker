use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Create a connection pool and run all pending migrations.
pub async fn init(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .context("connecting to PostgreSQL")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running database migrations")?;

    Ok(pool)
}
