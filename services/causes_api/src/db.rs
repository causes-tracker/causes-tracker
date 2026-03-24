use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Create a connection pool.
#[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
pub async fn init(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .context("connecting to PostgreSQL")?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connects() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = init(&url).await.expect("db::init failed");
        pool.close().await;
    }
}
