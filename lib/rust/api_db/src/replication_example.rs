//! Scaffolding for the replication protocol.
//!
//! A minimal concrete resource type used to exercise the replication protocol
//! plumbing before any real resource types (Plans etc.) are implemented.
//! This module will be removed once real resources exist.

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
