use std::num::NonZeroU64;

use anyhow::Context;
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

// ── LocalTxnId ───────────────────────────────────────────────────────────

/// A position in a single instance's local commit order.
///
/// Used for both `local_version` (the txid that wrote or received a journal
/// row) and `watermark` (the xmin observed at INSERT time — a safe resume
/// point for replication).  Both fields carry the same kind of value: a
/// Postgres xid8 from `pg_current_xact_id()` / `pg_snapshot_xmin()`.
///
/// Always ≥ 3: Postgres reserves the low txids for its own use —
/// 0 = InvalidTransactionId, 1 = BootstrapTransactionId,
/// 2 = FrozenTransactionId, 3 = FirstNormalTransactionId.
/// Any journal row the application writes has txid ≥ 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalTxnId(u64);

impl LocalTxnId {
    /// Smallest valid application txid (Postgres' `FirstNormalTransactionId`).
    pub const MIN: u64 = 3;

    pub fn new(v: u64) -> anyhow::Result<Self> {
        anyhow::ensure!(
            v >= Self::MIN,
            "LocalTxnId must be ≥ {} (Postgres reserves 0..=2)",
            Self::MIN,
        );
        Ok(Self(v))
    }

    /// Convert from a signed bigint (the Postgres transport shape).
    pub fn from_i64(v: i64) -> anyhow::Result<Self> {
        let u: u64 = v.try_into().context("LocalTxnId out of range")?;
        Self::new(u)
    }

    pub fn get(&self) -> u64 {
        self.0
    }

    /// Signed-bigint transport shape for sqlx.
    pub fn as_i64(&self) -> i64 {
        // Always fits: LocalTxnId ≥ 3 and databases exhaust txids long
        // before approaching i64::MAX.
        self.0 as i64
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

// ── Per-resource journal table helpers ───────────────────────────────────
//
// Every resource type (Plan, Sign, Symptom, Comment, ...) has its own
// journal table.  All such tables share the same 14 meta columns and
// differ only in their payload columns.  The helpers below encode and
// decode the meta half so each resource module only has to write the
// payload-specific SQL.

/// Bind-param values for the 14 shared meta columns.
///
/// Use `from_entry` to build from typed values, then bind each field into
/// the per-table `INSERT`.  `sqlx::query!` will still check the SQL at
/// compile time — the helper only removes the repetitive encoding work.
pub struct JournalMetaParams<'a> {
    pub origin_instance_id: &'a str,
    pub origin_id: &'a str,
    pub version: i64,
    pub previous_origin_instance_id: Option<&'a str>,
    pub previous_origin_id: Option<&'a str>,
    pub previous_version: Option<i64>,
    pub kind: &'a str,
    pub at: chrono::DateTime<chrono::Utc>,
    pub author_instance_id: &'a str,
    pub author_local_id: &'a str,
    pub embargoed: bool,
    pub slug: &'a str,
    pub project_id: &'a str,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl<'a> JournalMetaParams<'a> {
    /// Borrow the fields of a typed `JournalEntryHeader` + `ResourceEntryMeta`
    /// as bind params.  Fails if the version integers do not fit in `i64`.
    pub fn from_entry(
        header: &'a JournalEntryHeader,
        meta: &'a ResourceEntryMeta,
    ) -> anyhow::Result<Self> {
        let version: i64 = header
            .version
            .version
            .get()
            .try_into()
            .context("version does not fit in i64")?;
        let (prev_instance, prev_id, prev_version) = match &header.previous_version {
            None => (None, None, None),
            Some(p) => {
                let v: i64 = p
                    .version
                    .get()
                    .try_into()
                    .context("previous_version does not fit in i64")?;
                (
                    Some(p.origin_instance_id.as_str()),
                    Some(p.origin_id.as_str()),
                    Some(v),
                )
            }
        };
        Ok(Self {
            origin_instance_id: header.version.origin_instance_id.as_str(),
            origin_id: header.version.origin_id.as_str(),
            version,
            previous_origin_instance_id: prev_instance,
            previous_origin_id: prev_id,
            previous_version: prev_version,
            kind: header.kind.as_str(),
            at: header.at,
            author_instance_id: header.author.instance_id.as_str(),
            author_local_id: header.author.local_id.as_str(),
            embargoed: header.embargoed,
            slug: meta.slug.as_str(),
            project_id: meta.project_id.as_str(),
            created_at: meta.created_at,
        })
    }
}

/// Owned row shape for the 14 shared meta columns.
///
/// Resource modules fill this from their `SELECT ...` row, build an
/// author-appropriate `FederatedIdentity` (the local_id discriminator —
/// User vs. ServiceAccount — depends on each table's schema), and call
/// `into_parts` to get back typed header + meta.
pub struct JournalMetaRow {
    pub origin_instance_id: String,
    pub origin_id: String,
    pub version: i64,
    pub previous_origin_instance_id: Option<String>,
    pub previous_origin_id: Option<String>,
    pub previous_version: Option<i64>,
    pub kind: String,
    pub at: chrono::DateTime<chrono::Utc>,
    pub author_instance_id: String,
    pub author_local_id: String,
    pub embargoed: bool,
    pub slug: String,
    pub project_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl JournalMetaRow {
    /// Convert to typed `JournalEntryHeader` + `ResourceEntryMeta`.
    /// The `author` parameter lets the caller resolve the local_id
    /// discriminator from its table's own schema (User vs. ServiceAccount).
    pub fn into_parts(
        self,
        author: FederatedIdentity,
    ) -> anyhow::Result<(JournalEntryHeader, ResourceEntryMeta)> {
        let version = FederatedVersion {
            origin_instance_id: InstanceId::from_raw(&self.origin_instance_id)?,
            origin_id: OriginId::from_raw(&self.origin_id)?,
            version: NonZeroU64::new(self.version.try_into().context("version out of range")?)
                .context("version is zero")?,
        };
        let previous_version = match (
            self.previous_origin_instance_id,
            self.previous_origin_id,
            self.previous_version,
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
        let header = JournalEntryHeader {
            kind: self.kind.parse()?,
            at: self.at,
            author,
            version,
            previous_version,
            embargoed: self.embargoed,
        };
        let meta = ResourceEntryMeta {
            slug: Slug::new(self.slug)?,
            project_id: crate::role::ProjectId::new(self.project_id)?,
            created_at: self.created_at,
        };
        Ok((header, meta))
    }
}

/// Comma-separated list of the 14 shared meta columns, in canonical order.
/// Paste into `SELECT` lists and `INSERT` column lists to keep per-resource
/// SQL shorter and aligned with `JournalMetaRow`/`JournalMetaParams`.
pub const JOURNAL_META_COLUMNS: &str = "origin_instance_id, origin_id, version, \
     previous_origin_instance_id, previous_origin_id, previous_version, \
     kind, at, author_instance_id, author_local_id, embargoed, \
     slug, project_id, created_at";

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
    fn local_txn_id_rejects_reserved_values() {
        for v in 0..LocalTxnId::MIN {
            assert!(
                LocalTxnId::new(v).is_err(),
                "LocalTxnId::new({v}) should reject reserved txid",
            );
        }
    }

    #[test]
    fn local_txn_id_accepts_first_normal_txid() {
        let id = LocalTxnId::new(LocalTxnId::MIN).unwrap();
        assert_eq!(id.get(), LocalTxnId::MIN);
    }

    #[test]
    fn local_txn_id_i64_round_trip() {
        let id = LocalTxnId::new(42).unwrap();
        let back = LocalTxnId::from_i64(id.as_i64()).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn local_txn_id_from_i64_rejects_negative() {
        assert!(LocalTxnId::from_i64(-1).is_err());
    }

    #[test]
    fn local_txn_id_from_i64_rejects_reserved() {
        for v in 0..(LocalTxnId::MIN as i64) {
            assert!(LocalTxnId::from_i64(v).is_err());
        }
    }

    #[test]
    fn local_txn_id_ordering() {
        let a = LocalTxnId::new(10).unwrap();
        let b = LocalTxnId::new(20).unwrap();
        assert!(a < b);
        assert_eq!(a.min(b), a);
    }

    // ── meta encode/decode helpers ──────────────────────────────────────

    fn sample_entry() -> (JournalEntryHeader, ResourceEntryMeta) {
        let origin = InstanceId::from_raw(&Uuid::new_v4().to_string()).unwrap();
        let user = crate::admin::UserId::new();
        let header = JournalEntryHeader {
            kind: JournalKind::Entry,
            at: chrono::Utc::now(),
            author: FederatedIdentity {
                instance_id: origin.clone(),
                local_id: LocalId::User(user),
            },
            version: FederatedVersion {
                origin_instance_id: origin.clone(),
                origin_id: OriginId::new(),
                version: NonZeroU64::new(42).unwrap(),
            },
            previous_version: None,
            embargoed: false,
        };
        let meta = ResourceEntryMeta {
            slug: Slug::new("hello").unwrap(),
            project_id: crate::role::ProjectId::new(Uuid::new_v4().to_string()).unwrap(),
            created_at: chrono::Utc::now(),
        };
        (header, meta)
    }

    #[test]
    fn meta_params_roundtrip_through_row() {
        let (header, meta) = sample_entry();
        let p = JournalMetaParams::from_entry(&header, &meta).unwrap();

        // Pretend a DB round-trip by copying the params into an owned row
        // with the same shape the SELECT would produce.
        let row = JournalMetaRow {
            origin_instance_id: p.origin_instance_id.to_owned(),
            origin_id: p.origin_id.to_owned(),
            version: p.version,
            previous_origin_instance_id: p.previous_origin_instance_id.map(str::to_owned),
            previous_origin_id: p.previous_origin_id.map(str::to_owned),
            previous_version: p.previous_version,
            kind: p.kind.to_owned(),
            at: p.at,
            author_instance_id: p.author_instance_id.to_owned(),
            author_local_id: p.author_local_id.to_owned(),
            embargoed: p.embargoed,
            slug: p.slug.to_owned(),
            project_id: p.project_id.to_owned(),
            created_at: p.created_at,
        };

        let author = header.author.clone();
        let (decoded_header, decoded_meta) = row.into_parts(author).unwrap();
        assert_eq!(decoded_header.version, header.version);
        assert_eq!(decoded_header.kind, header.kind);
        assert_eq!(decoded_header.embargoed, header.embargoed);
        assert_eq!(decoded_header.previous_version, header.previous_version);
        assert_eq!(decoded_meta.slug, meta.slug);
        assert_eq!(decoded_meta.project_id, meta.project_id);
    }

    #[test]
    fn meta_params_encodes_previous_version_triple() {
        let (mut header, meta) = sample_entry();
        let prev = FederatedVersion {
            origin_instance_id: header.version.origin_instance_id.clone(),
            origin_id: OriginId::new(),
            version: NonZeroU64::new(7).unwrap(),
        };
        header.previous_version = Some(prev.clone());
        let p = JournalMetaParams::from_entry(&header, &meta).unwrap();
        assert_eq!(
            p.previous_origin_instance_id,
            Some(prev.origin_instance_id.as_str())
        );
        assert_eq!(p.previous_origin_id, Some(prev.origin_id.as_str()));
        assert_eq!(p.previous_version, Some(7));
    }

    #[test]
    fn meta_row_rejects_partial_previous_triple() {
        let (header, meta) = sample_entry();
        let p = JournalMetaParams::from_entry(&header, &meta).unwrap();
        let row = JournalMetaRow {
            origin_instance_id: p.origin_instance_id.to_owned(),
            origin_id: p.origin_id.to_owned(),
            version: p.version,
            // Partial: instance set but id and version missing.
            previous_origin_instance_id: Some("whatever".to_owned()),
            previous_origin_id: None,
            previous_version: None,
            kind: p.kind.to_owned(),
            at: p.at,
            author_instance_id: p.author_instance_id.to_owned(),
            author_local_id: p.author_local_id.to_owned(),
            embargoed: p.embargoed,
            slug: p.slug.to_owned(),
            project_id: p.project_id.to_owned(),
            created_at: p.created_at,
        };
        assert!(row.into_parts(header.author).is_err());
    }

    #[test]
    fn meta_row_rejects_zero_version() {
        let (header, meta) = sample_entry();
        let p = JournalMetaParams::from_entry(&header, &meta).unwrap();
        let row = JournalMetaRow {
            origin_instance_id: p.origin_instance_id.to_owned(),
            origin_id: p.origin_id.to_owned(),
            version: 0, // zero is reserved for the previous_version sentinel
            previous_origin_instance_id: None,
            previous_origin_id: None,
            previous_version: None,
            kind: p.kind.to_owned(),
            at: p.at,
            author_instance_id: p.author_instance_id.to_owned(),
            author_local_id: p.author_local_id.to_owned(),
            embargoed: p.embargoed,
            slug: p.slug.to_owned(),
            project_id: p.project_id.to_owned(),
            created_at: p.created_at,
        };
        assert!(row.into_parts(header.author).is_err());
    }

    #[test]
    fn meta_columns_constant_lists_14_columns() {
        let count = JOURNAL_META_COLUMNS.split(',').count();
        assert_eq!(count, 14);
    }
}
