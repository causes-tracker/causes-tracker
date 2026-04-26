//! Scaffolding for the replication protocol.
//!
//! A minimal concrete resource type used to exercise the replication protocol
//! plumbing before any real resource types (Plans etc.) are implemented.
//! This module will be removed once real resources exist.
//!
//! Demonstrates the per-resource pattern: declare the struct, invoke
//! `journal_table!`, get `insert_entry` + `entries_since` for free.

use crate::journal::{JournalEntryHeader, ResourceEntryMeta};

/// A minimal concrete resource journal entry.
/// Embeds the standard journal header and resource meta, plus a trivial
/// payload field.
#[derive(Debug, Clone)]
pub struct ReplicationExample {
    pub header: JournalEntryHeader,
    pub meta: ResourceEntryMeta,
    pub payload: String,
}

api_db_macros::journal_table! {
    table = "replication_example_journal",
    rust = ReplicationExample,
    payload = {
        payload: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::UserId;
    use crate::db::DbPool;
    use crate::journal::{
        FederatedIdentity, FederatedVersion, InstanceId, JournalKind, LocalId, LocalTxnId,
        OriginId, Slug,
    };
    use crate::role::ProjectId;
    use crate::test_support::{seed_project, test_instance};
    use sqlx::types::chrono;
    use std::num::NonZeroU64;

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
