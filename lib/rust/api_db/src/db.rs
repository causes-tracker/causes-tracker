use anyhow::Context;
use db_pool::{DbPool, QueryAccess};

/// Embedded migrations, compiled from `migrations/` at build time.
pub(crate) static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Run all pending migrations.
#[tracing::instrument(skip(pool), fields(db.system = "postgresql"))]
pub async fn migrate(pool: &DbPool) -> anyhow::Result<()> {
    MIGRATIONS
        .run(&pool.pool())
        .await
        .context("running database migrations")
}

/// Return this instance's stable identity (UUID v4).
///
/// Generated once during migration 007 and stored in `instance_config`.
/// This value never changes for the lifetime of the database.
pub async fn instance_id(pool: &DbPool) -> anyhow::Result<String> {
    let row = sqlx::query_scalar!("SELECT value FROM instance_config WHERE key = 'instance_id'")
        .fetch_one(&pool.pool())
        .await
        .context("reading instance_id from instance_config")?;
    Ok(row)
}

/// Begin a transaction at REPEATABLE READ isolation. The journal trigger
/// from migration 009 rejects lower isolation; non-journal transactions
/// also use it for consistent snapshot reads. Call this instead of
/// `pool.pool().begin()`; clippy's `disallowed_methods` lint rejects
/// direct `.begin()` calls elsewhere.
#[allow(clippy::disallowed_methods)] // The one legitimate caller of sqlx::Pool::begin.
pub(crate) async fn begin_txn(
    pool: &DbPool,
) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
    let mut tx = pool.pool().begin().await.context("beginning transaction")?;
    sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .context("setting isolation level")?;
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty migrator — gives us a bare database from `#[sqlx::test]` so we
    /// can exercise `DbPool::connect` and `migrate` ourselves.
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
        migrate(&pool).await.expect("migrate failed");
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

    /// Verify that instance_id is generated during migration and is a valid UUID.
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn instance_id_is_generated(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let id = instance_id(&db).await.expect("instance_id failed");
        id.parse::<uuid::Uuid>()
            .expect("instance_id is not a valid UUID");
    }

    /// Verify that running migrations twice preserves the existing instance_id.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn instance_id_survives_migration_rerun(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);

        MIGRATIONS.run(&db.pool()).await.expect("first run failed");
        let original = instance_id(&db).await.expect("instance_id failed");

        MIGRATIONS.run(&db.pool()).await.expect("second run failed");
        let after = instance_id(&db).await.expect("instance_id failed");

        assert_eq!(original, after);
    }

    /// Verify that `begin_txn` opens a transaction at REPEATABLE READ.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn begin_txn_sets_repeatable_read(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let mut tx = begin_txn(&db).await.expect("begin_txn failed");
        let level: String =
            sqlx::query_scalar!("SELECT current_setting('transaction_isolation') AS \"v!\"")
                .fetch_one(&mut *tx)
                .await
                .expect("query failed");
        assert_eq!(level, "repeatable read");
    }
}
