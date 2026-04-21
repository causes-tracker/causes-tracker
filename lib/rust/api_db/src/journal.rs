use std::num::NonZeroU64;

use sqlx::types::chrono;
use uuid::Uuid;

use crate::admin::{ServiceAccountId, UserId};

// ── Identifier newtypes ──────────────────────────────────────────────────

/// Identifies a Causes instance (UUID v4).
/// Generated at first bootstrap; stable across domain changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
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

/// The stable per-resource identity; combined with `origin_instance_id` and
/// `version` forms a `FederatedVersion` that identifies a journal entry.
/// Per designdocs/Replication.md: "origin_id is a UUID that identifies the
/// resource.  It is assigned once when the resource is first created and
/// reused by all subsequent entries about that resource."
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OriginId(String);

impl OriginId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_raw(s: &str) -> anyhow::Result<Self> {
        s.parse::<Uuid>()
            .map_err(|e| anyhow::anyhow!("OriginId must be a valid UUID: {e}"))?;
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OriginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Slug ─────────────────────────────────────────────────────────────────

/// Human-readable resource identifier within a project.
/// URL single-path-segment safe: non-empty, ASCII, lowercase, composed of
/// `[a-z0-9_-]`.  May differ across journal entries for the same resource
/// (renames change the slug).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(String);

impl Slug {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        anyhow::ensure!(!s.is_empty(), "Slug must not be empty");
        anyhow::ensure!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
            "Slug must contain only lowercase letters, digits, '-', and '_'",
        );
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Slug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── LocalId ──────────────────────────────────────────────────────────────

/// The `local_id` half of a `FederatedIdentity`: either a `UserId` or a
/// `ServiceAccountId` on the named instance.  Proto cannot express the
/// union directly (it transmits a bare string); the caller constructs the
/// correct variant based on the context in which the identity appears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalId {
    User(UserId),
    ServiceAccount(ServiceAccountId),
}

impl LocalId {
    /// Borrow the underlying id as a string (proto/SQL transport form).
    pub fn as_str(&self) -> &str {
        match self {
            Self::User(u) => u.as_str(),
            Self::ServiceAccount(s) => s.as_str(),
        }
    }
}

// ── FederatedIdentity ────────────────────────────────────────────────────

/// Identifies a user or service account across instances.
///
/// `instance_id` is the instance where the user/account exists.
/// `local_id` is the UserId or ServiceAccountId on that instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedIdentity {
    pub instance_id: InstanceId,
    pub local_id: LocalId,
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
/// Used for cross-resource references and as the identity of a journal
/// entry itself.  Version is `NonZeroU64` — version 0 is reserved as a
/// root sentinel and is represented as `None` in `previous_version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedVersion {
    /// The instance that wrote this journal entry.
    pub origin_instance_id: InstanceId,
    /// The stable resource UUID.
    pub origin_id: OriginId,
    /// Version number assigned at commit time on the writing instance.
    pub version: NonZeroU64,
}

// ── JournalEntryHeader ───────────────────────────────────────────────────

/// Immutable distributed identity and metadata for a single journal entry.
/// Rust counterpart of the proto `JournalEntryHeader`.
#[derive(Debug, Clone)]
pub struct JournalEntryHeader {
    pub kind: JournalKind,
    pub at: chrono::DateTime<chrono::Utc>,
    pub author: FederatedIdentity,
    pub version: FederatedVersion,
    /// `None` for the first entry of a resource (creation).
    pub previous_version: Option<FederatedVersion>,
    pub embargoed: bool,
}

// ── ResourceEntryMeta ────────────────────────────────────────────────────

/// Resource-level metadata carried in every journal entry.
/// Rust counterpart of the proto `ResourceEntryMeta`.
#[derive(Debug, Clone)]
pub struct ResourceEntryMeta {
    /// Human-readable identifier within the project.
    /// Immutable within a single entry; may differ across entries (rename).
    pub slug: Slug,
    /// The project this resource is filed under on the origin instance.
    pub project_id: crate::role::ProjectId,
    /// Timestamp of the resource's creation, copied from the first entry.
    pub created_at: chrono::DateTime<chrono::Utc>,
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
}
