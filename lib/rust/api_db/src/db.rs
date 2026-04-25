use std::sync::Arc;

use anyhow::Context;
use arc_swap::ArcSwap;
use sqlx::postgres::PgPoolOptions;

use crate::iam::IamParams;

/// Embedded migrations, compiled from `migrations/` at build time.
pub(crate) static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opaque pool handle.
/// sqlx types do not cross this boundary.
///
/// In static mode (dev/test), holds a fixed pool created from a `DATABASE_URL`.
/// In IAM mode (production), holds a swappable pool that is periodically
/// recreated with a fresh IAM auth token.
#[derive(Clone)]
pub struct DbPool {
    inner: Arc<ArcSwap<sqlx::PgPool>>,
    iam: Option<IamState>,
}

/// State needed to refresh the pool with a fresh IAM token.
#[derive(Clone)]
struct IamState {
    params: IamParams,
    sdk_config: aws_types::SdkConfig,
}

impl DbPool {
    /// Create a connection pool from environment-style configuration.
    ///
    /// When `db_host` and `db_user` are both `Some`, uses IAM authentication
    /// (production path).  Otherwise falls back to `database_url` which must
    /// be `Some`.
    #[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
    pub async fn from_config(
        db_host: Option<&str>,
        db_user: Option<&str>,
        db_port: u16,
        database_url: Option<&str>,
    ) -> anyhow::Result<Self> {
        match (db_host, db_user) {
            (Some(host), Some(user)) => {
                tracing::info!("using IAM database authentication");
                let sdk_config =
                    aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                let params = IamParams::new(host.to_owned(), db_port, user.to_owned());
                Self::connect_iam(params, sdk_config).await
            }
            _ => {
                let url = database_url
                    .context("DATABASE_URL is required when DB_HOST/DB_USER are not set")?;
                Self::connect(url).await
            }
        }
    }

    /// Create a connection pool from a static database URL (dev/test path).
    #[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connecting to PostgreSQL")?;
        Ok(Self {
            inner: Arc::new(ArcSwap::from_pointee(pool)),
            iam: None,
        })
    }

    /// Create a connection pool using IAM authentication (production path).
    ///
    /// Generates a short-lived auth token and connects. The pool can later
    /// be refreshed by calling [`refresh`](Self::refresh).
    #[tracing::instrument(skip(sdk_config), fields(db.system = "postgresql"))]
    pub async fn connect_iam(
        params: IamParams,
        sdk_config: aws_types::SdkConfig,
    ) -> anyhow::Result<Self> {
        let pool = build_iam_pool(&params, &sdk_config).await?;
        Ok(Self {
            inner: Arc::new(ArcSwap::from_pointee(pool)),
            iam: Some(IamState { params, sdk_config }),
        })
    }

    /// Spawn a background task that periodically refreshes the IAM auth token.
    ///
    /// In static mode (no IAM params) returns `None`.
    /// In IAM mode it regenerates the token every 6 hours, builds a new pool,
    /// and atomically swaps it in.  Existing connections drain naturally.
    ///
    /// The caller owns the returned `JoinHandle`; dropping it does **not**
    /// cancel the task, but [`JoinHandle::abort`] will.
    pub fn start_background_refresh(&self) -> Option<tokio::task::JoinHandle<()>> {
        let iam = self.iam.clone()?;
        let inner = self.inner.clone();
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
            // Skip the immediate first tick — the pool was just created.
            interval.tick().await;
            loop {
                interval.tick().await;
                match build_iam_pool(&iam.params, &iam.sdk_config).await {
                    Ok(new_pool) => {
                        inner.store(Arc::new(new_pool));
                        tracing::info!("database pool refreshed with new IAM token");
                    }
                    Err(e) => {
                        tracing::warn!("database pool refresh failed: {e}");
                    }
                }
            }
        }))
    }

    /// Wrap an existing sqlx pool (used by `#[sqlx::test]` harness).
    #[cfg(test)]
    pub(crate) fn from_pool(pool: sqlx::PgPool) -> Self {
        Self {
            inner: Arc::new(ArcSwap::from_pointee(pool)),
            iam: None,
        }
    }

    /// Get the current connection pool.
    ///
    /// `sqlx::PgPool` is internally reference-counted, so cloning is cheap.
    /// In IAM mode the underlying pool may be swapped at any time; callers
    /// get a snapshot that remains valid until dropped.
    pub(crate) fn pool(&self) -> sqlx::PgPool {
        (**self.inner.load()).clone()
    }

    /// Begin a transaction at REPEATABLE READ isolation.
    /// All transactions in this codebase run at REPEATABLE READ — journal
    /// writes require it (the trigger from migration 009 rejects lower
    /// isolation) and non-journal transactions use it for consistent
    /// snapshot reads across multi-statement operations.  Call this instead
    /// of `pool().begin()`; clippy's `disallowed_methods` lint rejects
    /// direct `.begin()` calls elsewhere.
    #[allow(clippy::disallowed_methods)] // The one legitimate caller of sqlx::Pool::begin.
    pub(crate) async fn begin_txn(&self) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
        let mut tx = self.pool().begin().await.context("beginning transaction")?;
        sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .context("setting isolation level")?;
        Ok(tx)
    }

    /// Run all pending migrations.
    #[tracing::instrument(skip(self), fields(db.system = "postgresql"))]
    pub async fn migrate(&self) -> anyhow::Result<()> {
        MIGRATIONS
            .run(&self.pool())
            .await
            .context("running database migrations")
    }

    /// Return this instance's stable identity (UUID v4).
    ///
    /// Generated once during migration 007 and stored in `instance_config`.
    /// This value never changes for the lifetime of the database.
    pub async fn instance_id(&self) -> anyhow::Result<String> {
        let row =
            sqlx::query_scalar!("SELECT value FROM instance_config WHERE key = 'instance_id'")
                .fetch_one(&self.pool())
                .await
                .context("reading instance_id from instance_config")?;
        Ok(row)
    }
}

/// Build a new `PgPool` using a freshly generated IAM auth token.
async fn build_iam_pool(
    params: &IamParams,
    sdk_config: &aws_types::SdkConfig,
) -> anyhow::Result<sqlx::PgPool> {
    let token = params.generate_token(sdk_config).await?;

    let options = sqlx::postgres::PgConnectOptions::new()
        .host(&params.hostname)
        .port(params.port)
        .username(&params.username)
        .password(&token)
        .database("causes")
        .ssl_mode(sqlx::postgres::PgSslMode::Require);

    PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("connecting to PostgreSQL with IAM token")
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

    /// Verify that `start_background_refresh` returns `None` when the pool
    /// was created in static mode (no IAM params) — no task is spawned.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn background_refresh_is_noop_in_static_mode(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        assert!(db.start_background_refresh().is_none());
    }

    /// Verify that instance_id is generated during migration and is a valid UUID.
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn instance_id_is_generated(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let id = db.instance_id().await.expect("instance_id failed");
        id.parse::<uuid::Uuid>()
            .expect("instance_id is not a valid UUID");
    }

    /// Verify that running migrations twice preserves the existing instance_id.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn instance_id_survives_migration_rerun(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);

        MIGRATIONS.run(&db.pool()).await.expect("first run failed");
        let original = db.instance_id().await.expect("instance_id failed");

        MIGRATIONS.run(&db.pool()).await.expect("second run failed");
        let after = db.instance_id().await.expect("instance_id failed");

        assert_eq!(original, after);
    }

    /// Verify that `begin_txn` opens a transaction at REPEATABLE READ.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn begin_txn_sets_repeatable_read(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let mut tx = db.begin_txn().await.expect("begin_txn failed");
        let level: String =
            sqlx::query_scalar!("SELECT current_setting('transaction_isolation') AS \"v!\"")
                .fetch_one(&mut *tx)
                .await
                .expect("query failed");
        assert_eq!(level, "repeatable read");
    }
}
