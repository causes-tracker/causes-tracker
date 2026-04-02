use uuid::Uuid;

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
}
