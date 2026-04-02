use anyhow::Context;
use uuid::Uuid;

use crate::DbPool;
use crate::admin::UserId;

// ── Role ─────────────────────────────────────────────────────────────────

/// Predefined roles from ADR-010.
///
/// `authenticated` and `anonymous` are implicit (not stored in the database).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    InstanceAdmin,
    Developer,
    ProjectMaintainer,
    SecurityTeam,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InstanceAdmin => "instance-admin",
            Self::Developer => "developer",
            Self::ProjectMaintainer => "project-maintainer",
            Self::SecurityTeam => "security-team",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "instance-admin" => Ok(Self::InstanceAdmin),
            "developer" => Ok(Self::Developer),
            "project-maintainer" => Ok(Self::ProjectMaintainer),
            "security-team" => Ok(Self::SecurityTeam),
            _ => anyhow::bail!("unknown role: {s:?}"),
        }
    }
}

// ── ProjectId ────────────────────────────────────────────────────────────

/// Project identifier (UUID v4).
///
/// Instance-level scope is represented by `Option<ProjectId>` being `None`,
/// not by a sentinel value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    /// Wrap a project ID string. Must be a valid UUID.
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        anyhow::ensure!(!s.is_empty(), "ProjectId must not be empty");
        s.parse::<Uuid>()
            .map_err(|e| anyhow::anyhow!("ProjectId must be a valid UUID: {e}"))?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── DB helpers ───────────────────────────────────────────────────────────

/// Convert `Option<ProjectId>` to the DB string (empty string = instance-level).
fn project_id_to_db(project_id: &Option<ProjectId>) -> &str {
    match project_id {
        Some(pid) => pid.as_str(),
        None => "",
    }
}

/// Convert a DB string to `Option<ProjectId>` (empty string = None).
fn project_id_from_db(s: String) -> anyhow::Result<Option<ProjectId>> {
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ProjectId::new(s)?))
    }
}

// ── Public API ───────────────────────────────────────────────────────────

/// A role assigned to a user, possibly scoped to a project.
pub struct RoleAssignment {
    /// `None` means instance-level scope.
    pub project_id: Option<ProjectId>,
    pub role: Role,
}

/// Get all roles for a user (both instance-level and project-level).
pub async fn get_user_roles(
    pool: &DbPool,
    user_id: &UserId,
) -> anyhow::Result<Vec<RoleAssignment>> {
    let rows = sqlx::query!(
        "SELECT project_id, role FROM role_assignments WHERE user_id = $1",
        user_id.as_str(),
    )
    .fetch_all(&pool.0)
    .await
    .context("querying user roles")?;

    rows.into_iter()
        .map(|r| {
            Ok(RoleAssignment {
                project_id: project_id_from_db(r.project_id)?,
                role: r.role.parse()?,
            })
        })
        .collect()
}

/// Get roles for a user scoped to a specific project, including instance-level roles.
pub async fn get_user_project_roles(
    pool: &DbPool,
    user_id: &UserId,
    project_id: &ProjectId,
) -> anyhow::Result<Vec<Role>> {
    let rows = sqlx::query_scalar!(
        "SELECT role FROM role_assignments \
         WHERE user_id = $1 AND (project_id = '' OR project_id = $2)",
        user_id.as_str(),
        project_id.as_str(),
    )
    .fetch_all(&pool.0)
    .await
    .context("querying user project roles")?;

    rows.into_iter().map(|r| r.parse()).collect()
}

/// Assign a role to a user. Idempotent: does nothing if the assignment exists.
///
/// `project_id` is `None` for instance-level roles.
pub async fn assign_role(
    pool: &DbPool,
    user_id: &UserId,
    project_id: &Option<ProjectId>,
    role: Role,
) -> anyhow::Result<()> {
    sqlx::query!(
        "INSERT INTO role_assignments (user_id, project_id, role) \
         VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING",
        user_id.as_str(),
        project_id_to_db(project_id),
        role.as_str(),
    )
    .execute(&pool.0)
    .await
    .context("assigning role")?;

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Role ──────────────────────────────────────────────────────────

    #[test]
    fn role_round_trips_through_str() {
        for role in [
            Role::InstanceAdmin,
            Role::Developer,
            Role::ProjectMaintainer,
            Role::SecurityTeam,
        ] {
            let s = role.as_str();
            let parsed: Role = s.parse().unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn role_as_str_matches_db_values() {
        assert_eq!(Role::InstanceAdmin.as_str(), "instance-admin");
        assert_eq!(Role::Developer.as_str(), "developer");
        assert_eq!(Role::ProjectMaintainer.as_str(), "project-maintainer");
        assert_eq!(Role::SecurityTeam.as_str(), "security-team");
    }

    #[test]
    fn role_display_matches_as_str() {
        assert_eq!(Role::InstanceAdmin.to_string(), "instance-admin");
    }

    #[test]
    fn role_rejects_unknown() {
        assert!("admin".parse::<Role>().is_err());
        assert!("".parse::<Role>().is_err());
        assert!("INSTANCE-ADMIN".parse::<Role>().is_err());
    }

    // ── ProjectId ─────────────────────────────────────────────────────

    #[test]
    fn project_id_rejects_empty() {
        assert!(ProjectId::new("").is_err());
    }

    #[test]
    fn project_id_rejects_non_uuid() {
        assert!(ProjectId::new("not-a-uuid").is_err());
        assert!(ProjectId::new("12345").is_err());
    }

    #[test]
    fn project_id_accepts_valid_uuid() {
        let uuid = Uuid::new_v4().to_string();
        let id = ProjectId::new(&uuid).unwrap();
        assert_eq!(id.as_str(), uuid);
    }

    #[test]
    fn project_id_display() {
        let uuid = Uuid::new_v4().to_string();
        let id = ProjectId::new(&uuid).unwrap();
        assert_eq!(id.to_string(), uuid);
    }

    // ── DB-backed tests ──────────────────────────────────────────────

    use crate::admin::{AuthProvider, DisplayName, Email, Subject, create_admin};

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
    async fn get_user_roles_empty_for_new_user(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = crate::admin::create_user(
            &pool,
            &DisplayName::new("Nobody").unwrap(),
            &Email::new("nobody@example.com").unwrap(),
            &AuthProvider::new("accounts.google.com").unwrap(),
            &Subject::new("nobody-sub").unwrap(),
        )
        .await
        .unwrap();

        let roles = get_user_roles(&pool, &user_id).await.unwrap();
        assert!(roles.is_empty());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn get_user_roles_returns_admin_role(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        let roles = get_user_roles(&pool, &user_id).await.unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].role, Role::InstanceAdmin);
        assert!(roles[0].project_id.is_none());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn assign_and_get_role(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        assign_role(&pool, &user_id, &None, Role::Developer)
            .await
            .unwrap();

        let roles = get_user_roles(&pool, &user_id).await.unwrap();
        assert_eq!(roles.len(), 2);
        let role_set: std::collections::HashSet<Role> = roles.iter().map(|r| r.role).collect();
        assert!(role_set.contains(&Role::InstanceAdmin));
        assert!(role_set.contains(&Role::Developer));
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn assign_role_is_idempotent(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;

        assign_role(&pool, &user_id, &None, Role::Developer)
            .await
            .unwrap();
        assign_role(&pool, &user_id, &None, Role::Developer)
            .await
            .unwrap();

        let roles = get_user_roles(&pool, &user_id).await.unwrap();
        assert_eq!(roles.len(), 2); // admin + developer, not duplicated
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn get_user_project_roles_includes_instance_level(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;
        let project_id = ProjectId::new(Uuid::new_v4().to_string()).unwrap();

        assign_role(
            &pool,
            &user_id,
            &Some(project_id.clone()),
            Role::ProjectMaintainer,
        )
        .await
        .unwrap();

        let roles = get_user_project_roles(&pool, &user_id, &project_id)
            .await
            .unwrap();
        assert!(roles.contains(&Role::InstanceAdmin));
        assert!(roles.contains(&Role::ProjectMaintainer));
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn get_user_project_roles_excludes_other_projects(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let user_id = seed_admin(&pool).await;
        let project_a = ProjectId::new(Uuid::new_v4().to_string()).unwrap();
        let project_b = ProjectId::new(Uuid::new_v4().to_string()).unwrap();

        assign_role(&pool, &user_id, &Some(project_a), Role::ProjectMaintainer)
            .await
            .unwrap();

        let roles = get_user_project_roles(&pool, &user_id, &project_b)
            .await
            .unwrap();
        // Only instance-admin (instance-level), not project-maintainer from project_a
        assert_eq!(roles, vec![Role::InstanceAdmin]);
    }
}
