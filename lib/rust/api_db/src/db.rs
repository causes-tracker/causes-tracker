use anyhow::Context;
use sqlx::postgres::PgPoolOptions;

/// Embedded migrations, compiled from `migrations/` at build time.
pub(crate) static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opaque pool handle.
/// sqlx types do not cross this boundary.
#[derive(Clone)]
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
        MIGRATIONS
            .run(&self.0)
            .await
            .context("running database migrations")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty migrator — gives us a bare database from `#[sqlx::test]` so we
    /// can exercise `DbPool::connect` and `DbPool::migrate` ourselves.
    static EMPTY: sqlx::migrate::Migrator = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Borrowed(&[]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    };

    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn connect_and_migrate(pool: sqlx::PgPool) {
        let port: String = sqlx::query_scalar!("SELECT current_setting('port')::text AS port")
            .fetch_one(&pool)
            .await
            .expect("failed to query port")
            .expect("port was null");
        let db: String = sqlx::query_scalar!("SELECT current_database()::text AS db")
            .fetch_one(&pool)
            .await
            .expect("failed to query database name")
            .expect("database was null");
        let url = format!("postgresql://localhost:{port}/{db}");

        let pool = DbPool::connect(&url).await.expect("DbPool::connect failed");
        pool.migrate().await.expect("migrate failed");
    }

    /// Runs migrations against a real PostgreSQL instance and asserts that all
    /// expected tables exist.
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn migrations_create_all_tables(pool: sqlx::PgPool) {
        let tables: Vec<String> = sqlx::query_scalar!(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename"
        )
        .fetch_all(&pool)
        .await
        .expect("pg_tables query failed")
        .into_iter()
        .flatten()
        .collect();

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
