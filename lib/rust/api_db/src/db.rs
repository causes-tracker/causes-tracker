use crate::Pool;
use anyhow::Context;
use db_pool::QueryAccess;

/// Embedded migrations, compiled from `migrations/` at build time.
pub(crate) static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Run all pending migrations.
#[tracing::instrument(skip(pool), fields(db.system = "postgresql"))]
pub async fn migrate(pool: &Pool) -> anyhow::Result<()> {
    MIGRATIONS
        .run(&pool.pool())
        .await
        .context("running database migrations")
}

/// Return this instance's stable identity (UUID v4).
///
/// Generated once during migration 007 and stored in `instance_config`.
/// This value never changes for the lifetime of the database.
pub async fn instance_id(pool: &Pool) -> anyhow::Result<String> {
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
    pool: &Pool,
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
    /// can exercise `Pool::connect` and `migrate` ourselves.
    /// `Migrator::DEFAULT` already carries empty migrations and the standard
    /// flags, so we reuse it rather than spelling out the struct literal —
    /// that keeps us compiling across sqlx releases that add fields.
    static EMPTY: sqlx::migrate::Migrator = sqlx::migrate::Migrator::DEFAULT;

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

        let pool = Pool::connect(&url, crate::PoolState::default())
            .await
            .expect("Pool::connect failed");
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
        let db = Pool::from_pool(pool, crate::PoolState::default());
        let id = instance_id(&db).await.expect("instance_id failed");
        id.parse::<uuid::Uuid>()
            .expect("instance_id is not a valid UUID");
    }

    /// Verify that running migrations twice preserves the existing instance_id.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn instance_id_survives_migration_rerun(pool: sqlx::PgPool) {
        let db = Pool::from_pool(pool, crate::PoolState::default());

        MIGRATIONS.run(&db.pool()).await.expect("first run failed");
        let original = instance_id(&db).await.expect("instance_id failed");

        MIGRATIONS.run(&db.pool()).await.expect("second run failed");
        let after = instance_id(&db).await.expect("instance_id failed");

        assert_eq!(original, after);
    }

    /// Verify that `begin_txn` opens a transaction at REPEATABLE READ.
    #[sqlx::test(migrator = "crate::db::tests::EMPTY")]
    async fn begin_txn_sets_repeatable_read(pool: sqlx::PgPool) {
        let db = Pool::from_pool(pool, crate::PoolState::default());
        let mut tx = begin_txn(&db).await.expect("begin_txn failed");
        let level: String =
            sqlx::query_scalar!("SELECT current_setting('transaction_isolation') AS \"v!\"")
                .fetch_one(&mut *tx)
                .await
                .expect("query failed");
        assert_eq!(level, "repeatable read");
    }

    // ── journal_create_table() ──────────────────────────────────────────

    /// Helper: ask information_schema for the columns of a created table.
    async fn columns_of(pool: &sqlx::PgPool, table: &str) -> Vec<(String, String, String)> {
        sqlx::query!(
            "SELECT column_name, data_type, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 \
             ORDER BY ordinal_position",
            table,
        )
        .fetch_all(pool)
        .await
        .expect("columns_of failed")
        .into_iter()
        .map(|r| {
            (
                r.column_name.unwrap_or_default(),
                r.data_type.unwrap_or_default(),
                r.is_nullable.unwrap_or_default(),
            )
        })
        .collect()
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

        let db = Pool::from_pool(pool, crate::PoolState::default());
        let project_id = seed_project_row(&db.pool()).await;
        let oi = uuid::Uuid::new_v4().to_string();

        // Happy path under REPEATABLE READ via begin_txn.
        let mut tx = begin_txn(&db).await.unwrap();
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
        let mut tx = begin_txn(&db).await.unwrap();
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

        let db = Pool::from_pool(pool, crate::PoolState::default());
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
        let mut tx = begin_txn(&db).await.unwrap();
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
        let mut tx = begin_txn(&db).await.unwrap();
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
        let mut tx = begin_txn(&db).await.unwrap();
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
