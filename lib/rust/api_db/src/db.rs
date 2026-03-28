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

    /// Run all pending migrations.
    #[tracing::instrument(skip(self), fields(db.system = "postgresql"))]
    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.0)
            .await
            .context("running database migrations")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs migrations against a real PostgreSQL instance and asserts that all
    /// expected tables exist.
    #[tokio::test]
    async fn migrations_create_all_tables() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

        let pool = DbPool::connect(&url).await.expect("connect failed");
        pool.migrate().await.expect("migrate failed");

        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
        )
        .fetch_all(&pool.0)
        .await
        .expect("pg_tables query failed");

        for expected in [
            "instance_config",
            "users",
            "external_identities",
            "role_assignments",
        ] {
            assert!(
                tables.contains(&expected.to_string()),
                "missing table: {expected}"
            );
        }
    }
}
