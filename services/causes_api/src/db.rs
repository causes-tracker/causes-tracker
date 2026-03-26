use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Create a connection pool and run all pending migrations.
#[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs migrations against a real PostgreSQL instance and asserts that all
    /// expected tables exist.
    #[tokio::test]
    async fn migrations_create_all_tables() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

        let pool = init(&url).await.expect("db::init failed");

        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
        )
        .fetch_all(&pool)
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
