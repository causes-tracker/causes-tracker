use std::num::NonZeroU64;

use sqlx::types::chrono;
use uuid::Uuid;

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
/// Used for cross-resource references and as the identity of a journal
/// entry itself.  Version is `NonZeroU64` — version 0 is reserved as a
/// root sentinel and is represented as `None` in `previous_version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedVersion {
    /// The instance that wrote this journal entry.
    pub origin_instance_id: InstanceId,
    /// The stable resource UUID.
    pub origin_id: String,
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
    pub slug: String,
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
