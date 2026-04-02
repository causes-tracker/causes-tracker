use anyhow::Context;
use sqlx::types::chrono;
use uuid::Uuid;

use crate::DbPool;
use crate::admin::UserId;
use crate::role::{ProjectId, Role};

// ── Newtypes ─────────────────────────────────────────────────────────────

/// Project name: a slug (lowercase alphanumeric + hyphens, 2–64 chars).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectName(String);

impl ProjectName {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        anyhow::ensure!(s.len() >= 2, "ProjectName must be at least 2 characters");
        anyhow::ensure!(s.len() <= 64, "ProjectName must be at most 64 characters");
        anyhow::ensure!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "ProjectName must contain only lowercase letters, digits, and hyphens"
        );
        anyhow::ensure!(
            !s.starts_with('-'),
            "ProjectName must not start with a hyphen"
        );
        anyhow::ensure!(!s.ends_with('-'), "ProjectName must not end with a hyphen");
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Project visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectVisibility {
    Public,
    Private,
}

impl ProjectVisibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

impl std::str::FromStr for ProjectVisibility {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            _ => anyhow::bail!("unknown visibility: {s:?}"),
        }
    }
}

impl std::fmt::Display for ProjectVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A project row from the database.
pub struct ProjectRow {
    pub id: ProjectId,
    pub name: ProjectName,
    pub description: String,
    pub visibility: ProjectVisibility,
    pub embargoed_by_default: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ── Public API ───────────────────────────────────────────────────────────

/// Create a project and assign the creator as project-maintainer.
///
/// Atomic: both the project insert and the role assignment happen in a
/// single transaction.
pub async fn create_project(
    pool: &DbPool,
    name: &ProjectName,
    description: &str,
    visibility: ProjectVisibility,
    embargoed_by_default: bool,
    creator_user_id: &UserId,
) -> anyhow::Result<ProjectId> {
    let id = Uuid::new_v4().to_string();
    let mut tx = pool.0.begin().await.context("beginning transaction")?;

    sqlx::query!(
        "INSERT INTO projects (id, name, description, visibility, embargoed_by_default) \
         VALUES ($1, $2, $3, $4, $5)",
        &id,
        name.as_str(),
        description,
        visibility.as_str(),
        embargoed_by_default,
    )
    .execute(&mut *tx)
    .await
    .context("inserting project")?;

    sqlx::query!(
        "INSERT INTO role_assignments (user_id, project_id, role) VALUES ($1, $2, $3)",
        creator_user_id.as_str(),
        &id,
        Role::ProjectMaintainer.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("assigning project-maintainer to creator")?;

    tx.commit().await.context("committing transaction")?;

    ProjectId::new(id)
}

/// Look up a project by ID.
pub async fn get_project(
    pool: &DbPool,
    project_id: &ProjectId,
) -> anyhow::Result<Option<ProjectRow>> {
    let row = sqlx::query!(
        "SELECT id, name, description, visibility, embargoed_by_default, created_at \
         FROM projects WHERE id = $1",
        project_id.as_str(),
    )
    .fetch_optional(&pool.0)
    .await
    .context("looking up project")?;

    row.map(|r| {
        Ok(ProjectRow {
            id: ProjectId::new(r.id)?,
            name: ProjectName::new(r.name)?,
            visibility: r.visibility.parse()?,
            description: r.description,
            embargoed_by_default: r.embargoed_by_default,
            created_at: r.created_at,
        })
    })
    .transpose()
}

/// List all projects.
pub async fn list_projects(pool: &DbPool) -> anyhow::Result<Vec<ProjectRow>> {
    let rows = sqlx::query!(
        "SELECT id, name, description, visibility, embargoed_by_default, created_at \
         FROM projects ORDER BY name"
    )
    .fetch_all(&pool.0)
    .await
    .context("listing projects")?;

    rows.into_iter()
        .map(|r| {
            Ok(ProjectRow {
                id: ProjectId::new(r.id)?,
                name: ProjectName::new(r.name)?,
                visibility: r.visibility.parse()?,
                description: r.description,
                embargoed_by_default: r.embargoed_by_default,
                created_at: r.created_at,
            })
        })
        .collect()
}

/// Rename a project. Returns false if the project was not found.
pub async fn rename_project(
    pool: &DbPool,
    project_id: &ProjectId,
    new_name: &ProjectName,
) -> anyhow::Result<bool> {
    let result = sqlx::query!(
        "UPDATE projects SET name = $1 WHERE id = $2",
        new_name.as_str(),
        project_id.as_str(),
    )
    .execute(&pool.0)
    .await
    .context("renaming project")?;

    Ok(result.rows_affected() > 0)
}

/// Delete a project and its role assignments. Returns false if not found.
pub async fn delete_project(pool: &DbPool, project_id: &ProjectId) -> anyhow::Result<bool> {
    let mut tx = pool.0.begin().await.context("beginning transaction")?;

    sqlx::query!(
        "DELETE FROM role_assignments WHERE project_id = $1",
        project_id.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("deleting project role assignments")?;

    let result = sqlx::query!("DELETE FROM projects WHERE id = $1", project_id.as_str(),)
        .execute(&mut *tx)
        .await
        .context("deleting project")?;

    tx.commit().await.context("committing transaction")?;

    Ok(result.rows_affected() > 0)
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{AuthProvider, DisplayName, Email, Subject, create_admin};
    use crate::role::get_user_roles;

    // ── ProjectName ──────────────────────────────────────────────────

    #[test]
    fn name_rejects_empty() {
        assert!(ProjectName::new("").is_err());
    }

    #[test]
    fn name_rejects_single_char() {
        assert!(ProjectName::new("a").is_err());
    }

    #[test]
    fn name_rejects_uppercase() {
        assert!(ProjectName::new("MyProject").is_err());
    }

    #[test]
    fn name_rejects_leading_hyphen() {
        assert!(ProjectName::new("-foo").is_err());
    }

    #[test]
    fn name_rejects_trailing_hyphen() {
        assert!(ProjectName::new("foo-").is_err());
    }

    #[test]
    fn name_rejects_over_64_chars() {
        assert!(ProjectName::new("a".repeat(65)).is_err());
    }

    #[test]
    fn name_accepts_valid_slugs() {
        assert!(ProjectName::new("my-project").is_ok());
        assert!(ProjectName::new("a1").is_ok());
        assert!(ProjectName::new("hello-world-123").is_ok());
        assert!(ProjectName::new("ab").is_ok());
    }

    // ── ProjectVisibility ────────────────────────────────────────────

    #[test]
    fn visibility_round_trips() {
        for v in [ProjectVisibility::Public, ProjectVisibility::Private] {
            let parsed: ProjectVisibility = v.as_str().parse().unwrap();
            assert_eq!(parsed, v);
        }
    }

    #[test]
    fn visibility_rejects_unknown() {
        assert!("internal".parse::<ProjectVisibility>().is_err());
    }

    // ── DB-backed tests ──────────────────────────────────────────────

    async fn seed_admin(pool: &DbPool) -> UserId {
        create_admin(
            pool,
            &DisplayName::new("Test Admin").unwrap(),
            &Email::new("admin@example.com").unwrap(),
            &AuthProvider::new("accounts.google.com").unwrap(),
            &Subject::new("test-sub-42").unwrap(),
        )
        .await
        .expect("seed_admin failed")
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_project_inserts_row_and_role(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        let project_id = create_project(
            &pool,
            &ProjectName::new("my-project").unwrap(),
            "A test project",
            ProjectVisibility::Private,
            false,
            &user_id,
        )
        .await
        .unwrap();

        let row = get_project(&pool, &project_id).await.unwrap().unwrap();
        assert_eq!(row.name.as_str(), "my-project");
        assert_eq!(row.description, "A test project");
        assert_eq!(row.visibility, ProjectVisibility::Private);
        assert!(!row.embargoed_by_default);

        // Creator should have project-maintainer role
        let roles = get_user_roles(&pool, &user_id).await.unwrap();
        let project_role = roles
            .iter()
            .find(|r| r.project_id.as_ref() == Some(&project_id));
        assert!(project_role.is_some());
        assert_eq!(project_role.unwrap().role, Role::ProjectMaintainer);
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_project_rejects_duplicate_name(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;
        let name = ProjectName::new("dupe").unwrap();

        create_project(
            &pool,
            &name,
            "",
            ProjectVisibility::Private,
            false,
            &user_id,
        )
        .await
        .unwrap();

        let err = create_project(
            &pool,
            &name,
            "",
            ProjectVisibility::Private,
            false,
            &user_id,
        )
        .await;
        assert!(err.is_err());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn get_project_returns_none_for_missing(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let pid = ProjectId::new(Uuid::new_v4().to_string()).unwrap();
        assert!(get_project(&pool, &pid).await.unwrap().is_none());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn rename_project_updates_name(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        let pid = create_project(
            &pool,
            &ProjectName::new("old-name").unwrap(),
            "",
            ProjectVisibility::Private,
            false,
            &user_id,
        )
        .await
        .unwrap();

        let renamed = rename_project(&pool, &pid, &ProjectName::new("new-name").unwrap())
            .await
            .unwrap();
        assert!(renamed);

        let row = get_project(&pool, &pid).await.unwrap().unwrap();
        assert_eq!(row.name.as_str(), "new-name");
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn rename_missing_returns_false(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let pid = ProjectId::new(Uuid::new_v4().to_string()).unwrap();
        let result = rename_project(&pool, &pid, &ProjectName::new("xx").unwrap())
            .await
            .unwrap();
        assert!(!result);
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn delete_project_removes_row_and_roles(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        let pid = create_project(
            &pool,
            &ProjectName::new("doomed").unwrap(),
            "",
            ProjectVisibility::Private,
            false,
            &user_id,
        )
        .await
        .unwrap();

        let deleted = delete_project(&pool, &pid).await.unwrap();
        assert!(deleted);
        assert!(get_project(&pool, &pid).await.unwrap().is_none());

        // Role assignments for this project should be gone
        let roles = get_user_roles(&pool, &user_id).await.unwrap();
        assert!(roles.iter().all(|r| r.project_id.as_ref() != Some(&pid)));
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn delete_missing_returns_false(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let pid = ProjectId::new(Uuid::new_v4().to_string()).unwrap();
        assert!(!delete_project(&pool, &pid).await.unwrap());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn list_projects_returns_all(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        create_project(
            &pool,
            &ProjectName::new("alpha").unwrap(),
            "",
            ProjectVisibility::Private,
            false,
            &user_id,
        )
        .await
        .unwrap();
        create_project(
            &pool,
            &ProjectName::new("beta").unwrap(),
            "",
            ProjectVisibility::Public,
            true,
            &user_id,
        )
        .await
        .unwrap();

        let projects = list_projects(&pool).await.unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name.as_str(), "alpha"); // ordered by name
        assert_eq!(projects[1].name.as_str(), "beta");
    }
}
