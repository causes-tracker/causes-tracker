//! Shared test fixtures for journal-table tests.
//!
//! Every per-resource journal table needs a `projects` row to satisfy its
//! foreign key, and a fresh `InstanceId` to author entries with.  This
//! module provides the two minimal helpers each test reaches for, so
//! resource modules don't keep redefining them.

use uuid::Uuid;

use crate::db::DbPool;
use crate::journal::InstanceId;
use crate::role::ProjectId;

/// A fresh randomized `InstanceId`, valid as a UUID.
pub(crate) fn test_instance() -> InstanceId {
    InstanceId::from_raw(&Uuid::new_v4().to_string()).unwrap()
}

/// Insert a minimal project row directly (bypassing `create_project`,
/// which requires a user) and return its id.  Use to satisfy the
/// `project_id` foreign key on journal tables in `#[sqlx::test]`-driven
/// tests.
pub(crate) async fn seed_project(pool: &DbPool) -> ProjectId {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_returns_distinct_uuids() {
        let a = test_instance();
        let b = test_instance();
        assert_ne!(a, b);
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn seed_project_inserts_a_row(pool: sqlx::PgPool) {
        let db = DbPool::from_pool(pool);
        let project = seed_project(&db).await;
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) AS \"c!\" FROM projects WHERE id = $1",
            project.as_str()
        )
        .fetch_one(&db.pool())
        .await
        .unwrap();
        assert_eq!(count, 1);
    }
}
