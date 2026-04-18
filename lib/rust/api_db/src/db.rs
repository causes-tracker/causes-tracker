use std::sync::Arc;

use anyhow::Context;
use arc_swap::ArcSwap;
use sqlx::postgres::PgPoolOptions;

use crate::iam::IamParams;

/// Embedded production migrations, compiled from `migrations/` at build time.
pub(crate) static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Composite migrator combining production migrations with test-only extras
/// from `migrations-test/`.  Use this as the `migrator` argument to
/// `#[sqlx::test]` when a test needs a table defined in `migrations-test/`
/// (e.g. `replication_example_journal`).  Production builds do not apply the
/// extras — only this composite does.
#[cfg(test)]
pub(crate) static TEST_MIGRATIONS: std::sync::LazyLock<sqlx::migrate::Migrator> =
    std::sync::LazyLock::new(|| {
        let extras: sqlx::migrate::Migrator = sqlx::migrate!("./migrations-test");
        let mut migrations: Vec<sqlx::migrate::Migration> =
            MIGRATIONS.migrations.iter().cloned().collect();
        migrations.extend(extras.migrations.iter().cloned());
        migrations.sort_by_key(|m| m.version);
        sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(migrations),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        }
    });

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

    // ── journal_create_table() ──────────────────────────────────────────

    /// Helper: ask information_schema for the columns of a created table.
    /// Runtime query — the row shape (3 TEXT cols) doesn't fit sqlx::query!'s
    /// preferred struct/scalar shapes, and the gain over a typed tuple is nil.
    #[allow(clippy::disallowed_methods)]
    async fn columns_of(pool: &sqlx::PgPool, table: &str) -> Vec<(String, String, String)> {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT column_name, data_type, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 \
             ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(pool)
        .await
        .expect("columns_of failed")
    }

    /// Insert a minimal projects row so journal-table FKs are satisfiable.
    async fn seed_project_row(pool: &sqlx::PgPool) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let name = format!("p-{}", &id[..8]);
        sqlx::query!(
            "INSERT INTO projects (id, name, visibility) VALUES ($1, $2, 'public')",
            id,
            name,
        )
        .execute(pool)
        .await
        .expect("seed project failed");
        id
    }

    /// `journal_create_table` produces a table whose meta columns match the
    /// canonical journal shape, plus the requested payload columns.
    ///
    /// Uses runtime sqlx::query because the table being inspected
    /// (`jct_demo`) is created at test time, not at sqlx prepare time.
    #[allow(clippy::disallowed_methods)]
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn journal_create_table_emits_canonical_shape(pool: sqlx::PgPool) {
        sqlx::query("SELECT journal_create_table('jct_demo', 'note TEXT NOT NULL')")
            .execute(&pool)
            .await
            .expect("journal_create_table call failed");

        let cols = columns_of(&pool, "jct_demo").await;
        let names: Vec<&str> = cols.iter().map(|(n, _, _)| n.as_str()).collect();

        for expected in [
            "origin_instance_id",
            "origin_id",
            "version",
            "previous_origin_instance_id",
            "previous_origin_id",
            "previous_version",
            "kind",
            "at",
            "author_instance_id",
            "author_local_id",
            "embargoed",
            "slug",
            "project_id",
            "created_at",
            "local_version",
            "watermark",
            "note", // payload column
        ] {
            assert!(names.contains(&expected), "missing column: {expected}");
        }
    }

    /// A row inserted into a function-created table outside REPEATABLE READ
    /// is rejected by the trigger that the function attaches.
    ///
    /// Runtime sqlx::query: `jct_iso` is created at test time.
    #[allow(clippy::disallowed_methods)]
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn journal_create_table_attaches_isolation_trigger(pool: sqlx::PgPool) {
        sqlx::query("SELECT journal_create_table('jct_iso', 'payload TEXT NOT NULL')")
            .execute(&pool)
            .await
            .expect("journal_create_table call failed");

        let project_id = seed_project_row(&pool).await;
        // Default sqlx connection is READ COMMITTED; trigger should reject.
        let err = sqlx::query(
            "INSERT INTO jct_iso (
                origin_instance_id, origin_id, version,
                kind, at, author_instance_id, author_local_id, embargoed,
                slug, project_id, created_at, payload
            ) VALUES ($1, $2, 100, 'entry', now(), $1, $1, false, 's', $3, now(), 'p')",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&project_id)
        .execute(&pool)
        .await
        .expect_err("INSERT outside REPEATABLE READ should be rejected");
        assert!(
            err.to_string().to_lowercase().contains("repeatable read"),
            "expected isolation error, got: {err}",
        );
    }

    /// Inserting valid rows under REPEATABLE READ succeeds, populates the
    /// replication-serving columns, and the previous_version constraint
    /// fires for partial triples.
    ///
    /// Runtime sqlx::query: `jct_rw` is created at test time.
    #[allow(clippy::disallowed_methods)]
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn journal_create_table_table_is_writable_under_rr(pool: sqlx::PgPool) {
        sqlx::query("SELECT journal_create_table('jct_rw', 'payload TEXT NOT NULL')")
            .execute(&pool)
            .await
            .expect("journal_create_table call failed");

        let db = DbPool::from_pool(pool);
        let project_id = seed_project_row(&db.pool()).await;
        let oi = uuid::Uuid::new_v4().to_string();

        // Happy path under REPEATABLE READ via begin_txn.
        let mut tx = db.begin_txn().await.unwrap();
        sqlx::query(
            "INSERT INTO jct_rw (
                origin_instance_id, origin_id, version,
                kind, at, author_instance_id, author_local_id, embargoed,
                slug, project_id, created_at, payload
            ) VALUES ($1, $2, 100, 'entry', now(), $1, $1, false, 's', $3, now(), 'p')",
        )
        .bind(&oi)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&project_id)
        .execute(&mut *tx)
        .await
        .expect("happy-path insert failed");
        tx.commit().await.unwrap();

        // local_version and watermark were assigned by DEFAULT.
        let (lv, wm): (i64, i64) = sqlx::query_as(
            "SELECT local_version, watermark FROM jct_rw WHERE origin_instance_id = $1",
        )
        .bind(&oi)
        .fetch_one(&db.pool())
        .await
        .unwrap();
        assert!(lv >= 3, "local_version should be a real txid");
        assert!(wm >= 3, "watermark should be a real txid");

        // Partial previous_version triple violates the CHECK constraint.
        let mut tx = db.begin_txn().await.unwrap();
        let err = sqlx::query(
            "INSERT INTO jct_rw (
                origin_instance_id, origin_id, version,
                previous_origin_instance_id, previous_origin_id, previous_version,
                kind, at, author_instance_id, author_local_id, embargoed,
                slug, project_id, created_at, payload
            ) VALUES ($1, $2, 200, $1, NULL, NULL, 'entry', now(), $1, $1, false, 's', $3, now(), 'p')",
        )
        .bind(&oi)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&project_id)
        .execute(&mut *tx)
        .await
        .expect_err("partial previous_version triple should violate check");
        assert!(
            err.to_string().contains("prev_all_or_none"),
            "expected check-constraint error, got: {err}",
        );
    }

    /// A resource type can carry an *optional* reference to another
    /// resource — a federated version triple (instance, origin, version).
    /// The pattern: three nullable columns with an all-or-none CHECK,
    /// plus a partial index for efficient reverse lookups (e.g. "all
    /// comments referring to plan version X").
    ///
    /// `journal_create_table()` is unopinionated about payload, so the
    /// migration just lists the ref columns + CHECK in the payload spec
    /// and adds the partial index in a follow-up statement.  This test
    /// proves all three insert shapes behave correctly.
    ///
    /// Runtime sqlx::query: `jct_with_ref` is created at test time.
    #[allow(clippy::disallowed_methods)]
    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn journal_create_table_supports_optional_resource_reference(pool: sqlx::PgPool) {
        sqlx::query(
            "SELECT journal_create_table(
                'jct_with_ref',
                'body                 TEXT NOT NULL,
                 ref_origin_instance_id TEXT,
                 ref_origin_id          TEXT,
                 ref_version            BIGINT,
                 CONSTRAINT jct_with_ref_ref_all_or_none CHECK (
                     (ref_origin_instance_id IS NULL
                         AND ref_origin_id IS NULL
                         AND ref_version IS NULL)
                     OR
                     (ref_origin_instance_id IS NOT NULL
                         AND ref_origin_id IS NOT NULL
                         AND ref_version IS NOT NULL)
                 )'
            )",
        )
        .execute(&pool)
        .await
        .expect("journal_create_table call failed");

        // Partial index for reverse lookup: only rows with a reference
        // appear in it.  WHERE ... IS NOT NULL is what makes it disjoint.
        sqlx::query(
            "CREATE INDEX jct_with_ref_ref_idx
                 ON jct_with_ref (ref_origin_instance_id, ref_origin_id, ref_version)
                 WHERE ref_origin_instance_id IS NOT NULL",
        )
        .execute(&pool)
        .await
        .expect("partial index creation failed");

        let db = DbPool::from_pool(pool);
        let project_id = seed_project_row(&db.pool()).await;
        let oi = uuid::Uuid::new_v4().to_string();

        let insert_sql = "INSERT INTO jct_with_ref (
            origin_instance_id, origin_id, version,
            kind, at, author_instance_id, author_local_id, embargoed,
            slug, project_id, created_at,
            body, ref_origin_instance_id, ref_origin_id, ref_version
        ) VALUES ($1, $2, $3, 'entry', now(), $1, $1, false, 's', $4, now(),
                  'b', $5, $6, $7)";

        // Shape 1: no reference (all three NULL).  Permitted.
        let mut tx = db.begin_txn().await.unwrap();
        sqlx::query(insert_sql)
            .bind(&oi)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(100_i64)
            .bind(&project_id)
            .bind(None::<String>)
            .bind(None::<String>)
            .bind(None::<i64>)
            .execute(&mut *tx)
            .await
            .expect("no-reference insert should succeed");
        tx.commit().await.unwrap();

        // Shape 2: full reference (all three non-NULL).  Permitted.
        let target_instance = uuid::Uuid::new_v4().to_string();
        let target_origin = uuid::Uuid::new_v4().to_string();
        let mut tx = db.begin_txn().await.unwrap();
        sqlx::query(insert_sql)
            .bind(&oi)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(200_i64)
            .bind(&project_id)
            .bind(Some(&target_instance))
            .bind(Some(&target_origin))
            .bind(Some(7_i64))
            .execute(&mut *tx)
            .await
            .expect("full-reference insert should succeed");
        tx.commit().await.unwrap();

        // Shape 3: partial reference.  Rejected by the CHECK constraint.
        let mut tx = db.begin_txn().await.unwrap();
        let err = sqlx::query(insert_sql)
            .bind(&oi)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(300_i64)
            .bind(&project_id)
            .bind(Some(&target_instance))
            .bind(None::<String>)
            .bind(Some(7_i64))
            .execute(&mut *tx)
            .await
            .expect_err("partial reference triple should violate check");
        assert!(
            err.to_string().contains("ref_all_or_none"),
            "expected check-constraint error, got: {err}",
        );

        // The partial index reflects only rows with a reference.
        let indexed_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM jct_with_ref
                 WHERE ref_origin_instance_id = $1
                   AND ref_origin_id = $2
                   AND ref_version = $3",
        )
        .bind(&target_instance)
        .bind(&target_origin)
        .bind(7_i64)
        .fetch_one(&db.pool())
        .await
        .unwrap();
        assert_eq!(indexed_rows, 1, "the one full-reference row is reachable");
    }
}
