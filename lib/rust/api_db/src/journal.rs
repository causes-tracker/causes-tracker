use sqlx::types::chrono;
use uuid::Uuid;

/// Version 0 is reserved and never assigned.
/// It is the root of every `previous_version` chain.
pub const VERSION_ROOT: i64 = 0;

// ── Identifier newtypes ──────────────────────────────────────────────────

/// Identifies a Causes instance (UUID v4).
/// Generated at first bootstrap; stable across domain changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    /// Generate a new random instance identifier.
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Parse and validate a raw string as a UUID.
    pub fn from_raw(s: &str) -> anyhow::Result<Self> {
        s.parse::<Uuid>()
            .map_err(|e| anyhow::anyhow!("InstanceId must be a valid UUID: {e}"))?;
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── ResourceOrigin ───────────────────────────────────────────────────────

/// Identifies the writing instance and the resource for a journal entry.
///
/// `instance_id` is the instance that wrote the entry (different entries
/// for the same resource may have different values).
/// `id` is the stable resource UUID, assigned once at creation and reused
/// by all subsequent entries regardless of which instance writes them.
/// Always present — local resources use the local instance's own instance_id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceOrigin {
    pub instance_id: InstanceId,
    pub id: String,
}

// ── FederatedIdentity ────────────────────────────────────────────────────

/// Identifies a user or service account across instances.
///
/// `instance_id` is the instance where the user/account exists.
/// `local_id` is the UserId or ServiceAccountId on that instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedIdentity {
    pub instance_id: InstanceId,
    pub local_id: String,
}

// ── Kind ─────────────────────────────────────────────────────────────────

/// Whether a journal entry represents a live resource or a deletion.
///
/// Whether an entry is a creation, update, rename, or undelete is derivable
/// from context (previous_version and slug comparison), not stored explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalKind {
    Entry,
    Tombstone,
}

impl JournalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Tombstone => "tombstone",
        }
    }
}

impl std::fmt::Display for JournalKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for JournalKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "entry" => Ok(Self::Entry),
            "tombstone" => Ok(Self::Tombstone),
            _ => anyhow::bail!("unknown journal kind: {s:?}"),
        }
    }
}

// ── FederatedVersion ─────────────────────────────────────────────────────

/// Uniquely identifies a single journal entry across the federation.
///
/// Used for `previous_version` links, cross-resource references, and as the
/// identity of a journal entry itself.
/// Version 0 means "no prior entry" (root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedVersion {
    pub origin: ResourceOrigin,
    pub version: i64,
}

impl FederatedVersion {
    /// The root version — no prior entry exists.
    pub fn root() -> Self {
        Self {
            origin: ResourceOrigin {
                instance_id: InstanceId::from_raw("00000000-0000-0000-0000-000000000000")
                    .expect("nil UUID is valid"),
                id: String::new(),
            },
            version: VERSION_ROOT,
        }
    }

    /// Whether this is the root (no prior entry).
    pub fn is_root(&self) -> bool {
        self.version == VERSION_ROOT
    }
}

// ── JournalEntryMeta ─────────────────────────────────────────────────────

/// Common metadata fields present on every journal entry row.
///
/// This is the Rust representation of `JournalEntryHeader` +
/// `ResourceEntryMeta` from the proto, combined for convenience
/// when working with database rows.
#[derive(Debug, Clone)]
pub struct JournalEntryMeta {
    // -- JournalEntryHeader fields --
    pub kind: JournalKind,
    pub at: chrono::DateTime<chrono::Utc>,
    pub author: FederatedIdentity,
    pub version: i64,
    pub previous_version: FederatedVersion,
    pub embargoed: bool,
    pub origin: ResourceOrigin,

    // -- ResourceEntryMeta fields --
    pub slug: String,
    pub project_id: crate::role::ProjectId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl JournalEntryMeta {
    /// Whether this entry represents a deleted resource.
    pub fn is_tombstone(&self) -> bool {
        self.kind == JournalKind::Tombstone
    }

    /// Whether this is the first entry for a resource (creation).
    pub fn is_creation(&self) -> bool {
        self.previous_version.is_root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trip() {
        for kind in [JournalKind::Entry, JournalKind::Tombstone] {
            let s = kind.as_str();
            let parsed: JournalKind = s.parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn kind_rejects_unknown() {
        assert!("delete".parse::<JournalKind>().is_err());
        assert!("create".parse::<JournalKind>().is_err());
        assert!("".parse::<JournalKind>().is_err());
    }

    #[test]
    fn kind_display() {
        assert_eq!(format!("{}", JournalKind::Entry), "entry");
        assert_eq!(format!("{}", JournalKind::Tombstone), "tombstone");
    }

    #[test]
    fn federated_version_root() {
        let root = FederatedVersion::root();
        assert!(root.is_root());
        assert_eq!(root.version, VERSION_ROOT);
    }

    #[test]
    fn federated_version_non_root() {
        let v = FederatedVersion {
            origin: ResourceOrigin {
                instance_id: InstanceId::generate(),
                id: "some-resource".to_string(),
            },
            version: 42,
        };
        assert!(!v.is_root());
    }

    #[test]
    fn version_root_is_zero() {
        assert_eq!(VERSION_ROOT, 0);
    }

    fn test_origin() -> ResourceOrigin {
        ResourceOrigin {
            instance_id: InstanceId::generate(),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    fn test_author() -> FederatedIdentity {
        FederatedIdentity {
            instance_id: InstanceId::generate(),
            local_id: "user-1".to_string(),
        }
    }

    #[test]
    fn journal_entry_meta_is_creation() {
        let meta = JournalEntryMeta {
            kind: JournalKind::Entry,
            at: chrono::Utc::now(),
            author: test_author(),
            version: 1,
            previous_version: FederatedVersion::root(),
            embargoed: false,
            origin: test_origin(),
            slug: "my-plan".to_string(),
            project_id: crate::role::ProjectId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
            created_at: chrono::Utc::now(),
        };
        assert!(meta.is_creation());
        assert!(!meta.is_tombstone());
    }

    #[test]
    fn journal_entry_meta_is_tombstone() {
        let origin = test_origin();
        let meta = JournalEntryMeta {
            kind: JournalKind::Tombstone,
            at: chrono::Utc::now(),
            author: test_author(),
            version: 2,
            previous_version: FederatedVersion {
                origin: origin.clone(),
                version: 1,
            },
            embargoed: false,
            origin,
            slug: "my-plan".to_string(),
            project_id: crate::role::ProjectId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
            created_at: chrono::Utc::now(),
        };
        assert!(meta.is_tombstone());
        assert!(!meta.is_creation());
    }
}
