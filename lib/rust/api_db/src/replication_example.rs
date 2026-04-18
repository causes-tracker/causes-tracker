//! Scaffolding for the replication protocol.
//!
//! A minimal concrete resource type used to exercise the replication protocol
//! plumbing before any real resource types (Plans etc.) are implemented.
//! This module will be removed once real resources exist.

use std::num::NonZeroU64;

use anyhow::Context;

use crate::admin::UserId;
use crate::db::DbPool;
use crate::journal::{
    FederatedIdentity, FederatedVersion, InstanceId, JournalEntryHeader, LocalId, LocalTxnId,
    OriginId, ResourceEntryMeta, Slug,
};
use crate::role::ProjectId;

/// A minimal concrete resource journal entry.
/// Embeds the standard journal header and resource meta, plus a trivial
/// payload field.
#[derive(Debug, Clone)]
pub struct ReplicationExample {
    pub header: JournalEntryHeader,
    pub meta: ResourceEntryMeta,
    pub payload: String,
}

/// Insert a journal entry into `replication_example_journal`.
///
/// Runs in a REPEATABLE READ transaction (required by the replication
/// protocol's causal ordering guarantee, enforced by the table trigger).
/// `local_version` and `watermark` are assigned by the database via DEFAULT
/// expressions.
pub async fn insert_entry(pool: &DbPool, entry: &ReplicationExample) -> anyhow::Result<()> {
    let version_i64: i64 = entry
        .header
        .version
        .version
        .get()
        .try_into()
        .context("version does not fit in i64")?;

    let (prev_instance, prev_id, prev_version): (Option<String>, Option<String>, Option<i64>) =
        match &entry.header.previous_version {
            None => (None, None, None),
            Some(prev) => {
                let v: i64 = prev
                    .version
                    .get()
                    .try_into()
                    .context("previous_version does not fit in i64")?;
                (
                    Some(prev.origin_instance_id.as_str().to_owned()),
                    Some(prev.origin_id.as_str().to_owned()),
                    Some(v),
                )
            }
        };

    let mut tx = pool.begin_txn().await?;

    sqlx::query!(
        "INSERT INTO replication_example_journal (
            origin_instance_id, origin_id, version,
            previous_origin_instance_id, previous_origin_id, previous_version,
            kind, at, author_instance_id, author_local_id, embargoed,
            slug, project_id, created_at,
            payload
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
        entry.header.version.origin_instance_id.as_str(),
        entry.header.version.origin_id.as_str(),
        version_i64,
        prev_instance,
        prev_id,
        prev_version,
        entry.header.kind.as_str(),
        entry.header.at,
        entry.header.author.instance_id.as_str(),
        entry.header.author.local_id.as_str(),
        entry.header.embargoed,
        entry.meta.slug.as_str(),
        entry.meta.project_id.as_str(),
        entry.meta.created_at,
        entry.payload,
    )
    .execute(&mut *tx)
    .await
    .context("inserting replication_example journal entry")?;

    tx.commit().await.context("committing transaction")?;
    Ok(())
}

/// One row from `replication_example_journal` with its Postgres-specific
/// replication columns exposed.
#[derive(Debug, Clone)]
pub struct StoredEntry {
    pub entry: ReplicationExample,
    pub local_version: LocalTxnId,
    #[allow(dead_code)]
    // Exposed for receivers; the serving-query shape matches the protocol spec.
    pub watermark: LocalTxnId,
}

/// Fetch entries for a project with `local_version >= after_cursor`,
/// ordered by `local_version` ascending.
///
/// `after_cursor` is the last watermark the caller observed; pass `None`
/// on the first pull ("from the beginning").  Entries with `local_version`
/// equal to the cursor may be re-delivered (at-least-once); callers must
/// dedup by federated version.
pub async fn entries_since(
    pool: &DbPool,
    project_id: &ProjectId,
    after_cursor: Option<LocalTxnId>,
) -> anyhow::Result<Vec<StoredEntry>> {
    // Real txids are ≥ 3; a sentinel of 0 matches every row.
    let cursor_i64 = after_cursor.map(|c| c.as_i64()).unwrap_or(0);
    let rows = sqlx::query!(
        "SELECT origin_instance_id, origin_id, version,
                previous_origin_instance_id, previous_origin_id, previous_version,
                kind, at, author_instance_id, author_local_id, embargoed,
                slug, project_id, created_at,
                payload, local_version, watermark
         FROM replication_example_journal
         WHERE project_id = $1 AND local_version >= $2
         ORDER BY local_version",
        project_id.as_str(),
        cursor_i64,
    )
    .fetch_all(&pool.pool())
    .await
    .context("querying replication_example journal")?;

    rows.into_iter()
        .map(|r| {
            let version = FederatedVersion {
                origin_instance_id: InstanceId::from_raw(&r.origin_instance_id)?,
                origin_id: OriginId::from_raw(&r.origin_id)?,
                version: NonZeroU64::new(r.version.try_into().context("version out of range")?)
                    .context("version is zero")?,
            };

            let previous_version = match (
                r.previous_origin_instance_id,
                r.previous_origin_id,
                r.previous_version,
            ) {
                (None, None, None) => None,
                (Some(i), Some(id), Some(v)) => Some(FederatedVersion {
                    origin_instance_id: InstanceId::from_raw(&i)?,
                    origin_id: OriginId::from_raw(&id)?,
                    version: NonZeroU64::new(v.try_into().context("prev version out of range")?)
                        .context("previous_version is zero")?,
                }),
                _ => anyhow::bail!("partial previous_version triple"),
            };

            // The scaffolding table stores `author_local_id` as a bare string
            // with no discriminator for User vs ServiceAccount.  Real journal
            // tables will carry the discriminator; here we assume UserId.
            let header = JournalEntryHeader {
                kind: r.kind.parse()?,
                at: r.at,
                author: FederatedIdentity {
                    instance_id: InstanceId::from_raw(&r.author_instance_id)?,
                    local_id: LocalId::User(UserId::from_raw(&r.author_local_id)?),
                },
                version,
                previous_version,
                embargoed: r.embargoed,
            };
            let meta = ResourceEntryMeta {
                slug: Slug::new(r.slug)?,
                project_id: ProjectId::new(r.project_id)?,
                created_at: r.created_at,
            };

            Ok(StoredEntry {
                entry: ReplicationExample {
                    header,
                    meta,
                    payload: r.payload,
                },
                local_version: LocalTxnId::from_i64(r.local_version)?,
                watermark: LocalTxnId::from_i64(r.watermark)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::JournalKind;
    use sqlx::types::chrono;
    use uuid::Uuid;

    fn test_instance() -> InstanceId {
        InstanceId::from_raw(&Uuid::new_v4().to_string()).unwrap()
    }

    fn test_entry(
        instance: &InstanceId,
        resource: &OriginId,
        version: u64,
        project_id: &ProjectId,
        previous_version: Option<FederatedVersion>,
        payload: &str,
    ) -> ReplicationExample {
        ReplicationExample {
            header: JournalEntryHeader {
                kind: JournalKind::Entry,
                at: chrono::Utc::now(),
                author: FederatedIdentity {
                    instance_id: instance.clone(),
                    local_id: LocalId::User(UserId::new()),
                },
                version: FederatedVersion {
                    origin_instance_id: instance.clone(),
                    origin_id: resource.clone(),
                    version: NonZeroU64::new(version).unwrap(),
                },
                previous_version,
                embargoed: false,
            },
            meta: ResourceEntryMeta {
                slug: Slug::new("example").unwrap(),
                project_id: project_id.clone(),
                created_at: chrono::Utc::now(),
            },
            payload: payload.to_string(),
        }
    }

    async fn seed_project(pool: &DbPool) -> ProjectId {
        // Bypass the normal project creation (which requires a user) by
        // directly inserting a minimal project row for the test.
        let id = Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT INTO projects (id, name, visibility) VALUES ($1, $2, 'public')",
            id,
            format!("p-{}", &id[..8]),
        )
        .execute(&pool.pool())
        .await
        .expect("seed project");
        ProjectId::new(id).unwrap()
    }

    #[sqlx::test(migrator = "crate::db::TEST_MIGRATIONS")]
    async fn insert_and_read_back(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let project = seed_project(&db).await;
        let instance = test_instance();
        let res1 = OriginId::new();
        let entry = test_entry(&instance, &res1, 100, &project, None, "hello");

        insert_entry(&db, &entry).await.expect("insert failed");

        let stored = entries_since(&db, &project, None)
            .await
            .expect("read failed");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].entry.payload, "hello");
        assert_eq!(stored[0].entry.header.version.version.get(), 100);
        assert!(stored[0].local_version.get() > 0);
    }

    #[sqlx::test(migrator = "crate::db::TEST_MIGRATIONS")]
    async fn entries_since_filters_by_watermark(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let project = seed_project(&db).await;
        let instance = test_instance();

        let res1 = OriginId::new();
        let res2 = OriginId::new();
        let e1 = test_entry(&instance, &res1, 100, &project, None, "first");
        insert_entry(&db, &e1).await.unwrap();
        let first_lv = entries_since(&db, &project, None).await.unwrap()[0].local_version;

        let e2 = test_entry(&instance, &res2, 101, &project, None, "second");
        insert_entry(&db, &e2).await.unwrap();

        // Pass the first entry's local_version + 1 as the cursor — only e2 should remain.
        let next = LocalTxnId::new(first_lv.get() + 1).unwrap();
        let after = entries_since(&db, &project, Some(next)).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].entry.payload, "second");
    }

    #[sqlx::test(migrator = "crate::db::TEST_MIGRATIONS")]
    async fn previous_version_round_trip(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let project = seed_project(&db).await;
        let instance = test_instance();

        let res1 = OriginId::new();
        let e1 = test_entry(&instance, &res1, 100, &project, None, "v1");
        insert_entry(&db, &e1).await.unwrap();

        let prev = FederatedVersion {
            origin_instance_id: instance.clone(),
            origin_id: res1.clone(),
            version: NonZeroU64::new(100).unwrap(),
        };
        let e2 = test_entry(&instance, &res1, 200, &project, Some(prev.clone()), "v2");
        insert_entry(&db, &e2).await.unwrap();

        let stored = entries_since(&db, &project, None).await.unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored[0].entry.header.previous_version.is_none());
        assert_eq!(stored[1].entry.header.previous_version, Some(prev));
    }
}
