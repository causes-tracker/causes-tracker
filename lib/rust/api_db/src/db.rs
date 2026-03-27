use anyhow::Context;
use sqlx::postgres::PgPoolOptions;

/// Opaque pool handle.
/// sqlx types do not cross this boundary.
pub struct DbPool(pub(crate) sqlx::PgPool);

impl DbPool {
    /// Create a connection pool.
    #[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connecting to PostgreSQL")?;
        Ok(Self(pool))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_succeeds() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        DbPool::connect(&url).await.expect("connect failed");
    }
}
