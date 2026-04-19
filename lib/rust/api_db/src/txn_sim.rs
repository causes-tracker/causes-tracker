//! In-memory model of Postgres transaction-id and snapshot-xmin behaviour.
//!
//! Used to test that the replication protocol's watermark mechanism handles
//! out-of-order commits correctly without needing a real database.
//!
//! Models the subset of Postgres semantics that matters for journal writes:
//! - `pg_current_xact_id()` allocates a monotonically increasing txid at
//!   transaction start.
//! - `pg_snapshot_xmin(pg_current_snapshot())` returns the lowest still-running
//!   txid at the moment of the call.
//! - Inserts evaluated at INSERT time (the DEFAULT expression captures both).
//! - A row inserted by a transaction becomes visible to other readers only
//!   after the transaction commits.
//!
//! See designdocs/Replication.md for the protocol invariants this exercises.

#![allow(dead_code)] // scaffolding for tests

use std::collections::{BTreeSet, HashMap, HashSet};
use std::num::NonZeroU64;

use sqlx::types::chrono;

use crate::journal::{
    FederatedIdentity, FederatedVersion, InstanceId, JournalEntryHeader, JournalKind,
    ResourceEntryMeta,
};
use crate::replication_example::ReplicationExample;
use crate::role::ProjectId;

// ── Clock ────────────────────────────────────────────────────────────────

/// Models Postgres transaction ID allocation and snapshot xmin computation.
#[derive(Debug, Default)]
pub struct Clock {
    next_txid: u64,
    in_flight: BTreeSet<u64>,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            next_txid: 1,
            in_flight: BTreeSet::new(),
        }
    }

    /// Allocate a new txid and mark it in-flight.
    /// Equivalent to `pg_current_xact_id()` at the start of a transaction.
    pub fn begin(&mut self) -> u64 {
        let txid = self.next_txid;
        self.next_txid += 1;
        self.in_flight.insert(txid);
        txid
    }

    /// Return the lowest still-running txid.
    /// Equivalent to `pg_snapshot_xmin(pg_current_snapshot())`.
    /// If no transactions are in flight, returns `next_txid` (everything before
    /// is committed; nothing equal or greater exists yet).
    pub fn xmin(&self) -> u64 {
        self.in_flight.first().copied().unwrap_or(self.next_txid)
    }

    /// Mark a txid as committed (no longer in flight).
    pub fn commit(&mut self, txid: u64) {
        self.in_flight.remove(&txid);
    }

    /// Number of transactions currently in flight.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }
}

// ── TxnNode ──────────────────────────────────────────────────────────────

/// One stored entry, with the row visibility tracked separately.
#[derive(Debug, Clone)]
struct StoredRow {
    entry: ReplicationExample,
    /// txid of the transaction that wrote this row.
    local_version: u64,
    /// xmin observed at INSERT time (snapshot of in-flight set).
    watermark: u64,
    /// Whether the writing transaction has committed (and thus the row is
    /// visible to other readers).
    committed: bool,
}

type EntryKey = (InstanceId, String, u64);

fn entry_key(v: &FederatedVersion) -> EntryKey {
    (
        v.origin_instance_id.clone(),
        v.origin_id.clone(),
        v.version.get(),
    )
}

/// A node that uses a `Clock` to model concurrent transactions.
#[derive(Debug)]
pub struct TxnNode {
    instance_id: InstanceId,
    clock: Clock,
    /// All inserted rows. Each row carries its own visibility flag.
    rows: HashMap<EntryKey, StoredRow>,
    /// Pending inserts grouped by the writing txid (commits make them visible).
    pending_by_txid: HashMap<u64, Vec<EntryKey>>,
    /// Replication cursor per (upstream, project).
    cursors: HashMap<(InstanceId, ProjectId), u64>,
    /// Embargo trust (this node serves embargoed entries to these peers).
    serves_embargo_to: HashSet<InstanceId>,
}

impl TxnNode {
    pub fn new(instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            clock: Clock::new(),
            rows: HashMap::new(),
            pending_by_txid: HashMap::new(),
            cursors: HashMap::new(),
            serves_embargo_to: HashSet::new(),
        }
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn serve_embargo_to(&mut self, peer: &InstanceId) {
        self.serves_embargo_to.insert(peer.clone());
    }

    /// Begin a transaction, returning its txid. The caller passes this to
    /// subsequent `insert` calls and to `commit`.
    pub fn begin(&mut self) -> u64 {
        self.clock.begin()
    }

    /// Insert a row inside an open transaction.
    /// `local_version` = the transaction's txid.
    /// `watermark` = xmin at this moment (captured by the DEFAULT in real PG).
    pub fn insert(&mut self, txid: u64, args: InsertArgs) -> FederatedVersion {
        assert!(
            self.pending_by_txid.contains_key(&txid) || self.clock.in_flight.contains(&txid),
            "txid {txid} is not an open transaction",
        );

        let watermark = self.clock.xmin();

        let version = FederatedVersion {
            origin_instance_id: self.instance_id.clone(),
            origin_id: args.resource_id.to_string(),
            version: NonZeroU64::new(txid).unwrap(),
        };
        let header = JournalEntryHeader {
            kind: args.kind,
            at: chrono::Utc::now(),
            author: FederatedIdentity {
                instance_id: self.instance_id.clone(),
                local_id: "sim".to_string(),
            },
            version: version.clone(),
            previous_version: args.previous_version,
            embargoed: args.embargoed,
        };
        let meta = ResourceEntryMeta {
            slug: args.slug.to_string(),
            project_id: args.project_id.clone(),
            created_at: chrono::Utc::now(),
        };
        let entry = ReplicationExample {
            header,
            meta,
            payload: args.payload.to_string(),
        };

        let key = entry_key(&version);
        self.rows.insert(
            key.clone(),
            StoredRow {
                entry,
                local_version: txid,
                watermark,
                committed: false,
            },
        );
        self.pending_by_txid.entry(txid).or_default().push(key);
        version
    }

    /// Commit a transaction: make all its inserts visible.
    pub fn commit(&mut self, txid: u64) {
        self.clock.commit(txid);
        if let Some(keys) = self.pending_by_txid.remove(&txid) {
            for key in keys {
                if let Some(row) = self.rows.get_mut(&key) {
                    row.committed = true;
                }
            }
        }
    }

    /// Convenience: open a transaction, do one insert, commit immediately.
    pub fn auto_insert(&mut self, args: InsertArgs) -> FederatedVersion {
        let txid = self.begin();
        let v = self.insert(txid, args);
        self.commit(txid);
        v
    }

    pub fn count_committed(&self) -> usize {
        self.rows.values().filter(|r| r.committed).count()
    }

    pub fn has(&self, version: &FederatedVersion) -> bool {
        self.rows
            .get(&entry_key(version))
            .map(|r| r.committed)
            .unwrap_or(false)
    }

    /// Replication-serving query: entries visible (committed) with
    /// `local_version >= cursor`, ordered by `local_version`.
    fn visible_entries_for(&self, project_id: &ProjectId, cursor: u64) -> Vec<&StoredRow> {
        let mut rows: Vec<&StoredRow> = self
            .rows
            .values()
            .filter(|r| r.committed)
            .filter(|r| &r.entry.meta.project_id == project_id)
            .filter(|r| r.local_version >= cursor)
            .collect();
        rows.sort_by_key(|r| r.local_version);
        rows
    }

    /// Pull entries from `other` for `project_id`. Implements:
    /// - cursor at-least-once: use stored cursor as `>=` filter
    /// - dedup by federated version
    /// - embargo filtering
    /// - cursor wind-back: store the watermark of the last entry served
    pub fn replicate_from(&mut self, other: &TxnNode, project_id: &ProjectId) -> usize {
        let cursor_key = (other.instance_id.clone(), project_id.clone());
        let cursor = self.cursors.get(&cursor_key).copied().unwrap_or(0);

        let serve_embargoed = other.serves_embargo_to.contains(&self.instance_id);
        let to_send = other.visible_entries_for(project_id, cursor);

        let mut accepted = 0;
        let mut new_cursor = cursor;

        for row in to_send {
            // Wind cursor back to the watermark of the last entry served.
            // (Take the watermark even if we filter the entry out — the cursor
            // mechanism is independent of filtering decisions.)
            new_cursor = row.watermark;

            if row.entry.header.version.origin_instance_id == self.instance_id {
                continue;
            }
            if row.entry.header.embargoed && !serve_embargoed {
                continue;
            }
            let key = entry_key(&row.entry.header.version);
            if self.rows.contains_key(&key) {
                continue;
            }

            // Insert into self, autocommit.
            let receive_txid = self.clock.begin();
            self.rows.insert(
                key.clone(),
                StoredRow {
                    entry: row.entry.clone(),
                    local_version: receive_txid,
                    watermark: self.clock.xmin(),
                    committed: false,
                },
            );
            self.clock.commit(receive_txid);
            self.rows.get_mut(&key).unwrap().committed = true;
            accepted += 1;
        }

        self.cursors.insert(cursor_key, new_cursor);
        accepted
    }
}

// ── Argument structs ─────────────────────────────────────────────────────

pub struct InsertArgs<'a> {
    pub resource_id: &'a str,
    pub project_id: &'a ProjectId,
    pub slug: &'a str,
    pub payload: &'a str,
    pub embargoed: bool,
    pub kind: JournalKind,
    pub previous_version: Option<FederatedVersion>,
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication_sim::{named_instance, test_project};

    fn create_args<'a>(
        resource_id: &'a str,
        project_id: &'a ProjectId,
        slug: &'a str,
        payload: &'a str,
    ) -> InsertArgs<'a> {
        InsertArgs {
            resource_id,
            project_id,
            slug,
            payload,
            embargoed: false,
            kind: JournalKind::Entry,
            previous_version: None,
        }
    }

    // ── Clock unit tests ────────────────────────────────────────────────

    #[test]
    fn clock_alloc_and_xmin() {
        let mut c = Clock::new();
        let t1 = c.begin();
        assert_eq!(c.xmin(), t1);
        let t2 = c.begin();
        assert_eq!(c.xmin(), t1); // still t1
        c.commit(t1);
        assert_eq!(c.xmin(), t2);
        c.commit(t2);
        // No txns in flight, xmin returns next_txid (3).
        assert_eq!(c.xmin(), 3);
    }

    // ── Out-of-order commit semantics ───────────────────────────────────

    #[test]
    fn watermark_lags_under_long_running_txn() {
        let project = test_project();
        let mut a = TxnNode::new(named_instance("A"));

        // Open a long-running transaction T1.
        let t1 = a.begin();
        let v1 = a.insert(t1, create_args("res-1", &project, "s1", "from-t1"));

        // Many quick transactions commit while T1 is open.
        let mut later = Vec::new();
        for i in 0..5 {
            later.push(a.auto_insert(create_args(
                &format!("res-{}", i + 2),
                &project,
                &format!("s{}", i + 2),
                "p",
            )));
        }

        // T1 is still in-flight; row(T1) is not committed yet.
        assert!(!a.has(&v1));
        for v in &later {
            assert!(a.has(v));
        }

        // Crucially: each "later" entry's watermark equals T1's xmin (=t1)
        // because T1 is still in-flight at every later insert.
        for v in &later {
            let row = a.rows.get(&entry_key(v)).unwrap();
            assert_eq!(
                row.watermark, t1,
                "watermark on {:?} should equal T1's txid while T1 is in-flight",
                v
            );
        }

        // Now T1 commits.
        a.commit(t1);
        assert!(a.has(&v1));
    }

    /// Two transactions running concurrently, second commits first.
    /// A reader that pulls between commits sees only the second; after the
    /// first commits, the next pull (using the first response's watermark
    /// as cursor) must catch the late commit.
    #[test]
    fn out_of_order_commit_caught_by_watermark() {
        let project = test_project();
        let mut a = TxnNode::new(named_instance("A"));
        let mut b = TxnNode::new(named_instance("B"));

        // Open T1 (txid=1) on A.
        let t1 = a.begin();
        let v1 = a.insert(t1, create_args("res-1", &project, "s1", "t1-row"));

        // Open and commit T2 (txid=2) on A.
        let t2 = a.begin();
        let v2 = a.insert(t2, create_args("res-2", &project, "s2", "t2-row"));
        a.commit(t2);

        // B replicates from A. Only T2 is committed; B receives only v2.
        let n = b.replicate_from(&a, &project);
        assert_eq!(n, 1);
        assert!(b.has(&v2));
        assert!(!b.has(&v1));

        // Cursor on B for A must be set to row(t2)'s watermark, not t2.
        // row(t2)'s watermark should be t1 (because t1 was in-flight).
        let cursor = b
            .cursors
            .get(&(a.instance_id().clone(), project.clone()))
            .copied()
            .unwrap();
        assert_eq!(cursor, t1, "cursor must wind back to t1's watermark");

        // Now T1 commits.
        a.commit(t1);

        // B replicates again. Cursor=t1, query is local_version >= t1.
        // Both v1 and v2 match. v2 is dedup'd; v1 is new.
        let n = b.replicate_from(&a, &project);
        assert_eq!(n, 1);
        assert!(b.has(&v1));
    }

    /// The pathological "advance not winding back" anti-test:
    /// if we used `local_version` instead of `watermark` as the cursor,
    /// late-committing entries would be missed. This test asserts that
    /// our implementation does NOT do that — by checking every entry is
    /// eventually delivered.
    #[test]
    fn no_entries_lost_under_arbitrary_commit_order() {
        let project = test_project();
        let mut a = TxnNode::new(named_instance("A"));
        let mut b = TxnNode::new(named_instance("B"));

        // Open T1 first, then T2..T5; commit in reverse order.
        let t1 = a.begin();
        let v1 = a.insert(t1, create_args("res-1", &project, "s1", "v1"));
        let txns: Vec<(u64, FederatedVersion)> = (2..=5)
            .map(|i| {
                let txid = a.begin();
                let v = a.insert(
                    txid,
                    create_args(
                        &format!("res-{i}"),
                        &project,
                        &format!("s{i}"),
                        &format!("v{i}"),
                    ),
                );
                (txid, v)
            })
            .collect();

        // Commit in reverse: T5, T4, T3, T2, T1.
        // After each commit, replicate B from A.
        for (txid, _) in txns.iter().rev() {
            a.commit(*txid);
            b.replicate_from(&a, &project);
        }
        a.commit(t1);
        b.replicate_from(&a, &project);

        // All five entries should be on B.
        assert!(b.has(&v1));
        for (_, v) in &txns {
            assert!(b.has(v), "missing entry {:?}", v);
        }
    }

    /// Repeated pull while nothing changes returns no new entries (steady state).
    #[test]
    fn repeated_pull_is_steady_state() {
        let project = test_project();
        let mut a = TxnNode::new(named_instance("A"));
        let mut b = TxnNode::new(named_instance("B"));

        let _ = a.auto_insert(create_args("res-1", &project, "s1", "p"));
        assert_eq!(b.replicate_from(&a, &project), 1);
        assert_eq!(b.replicate_from(&a, &project), 0);
        assert_eq!(b.replicate_from(&a, &project), 0);
    }

    /// At-least-once semantics: an entry that lands at the same local_version
    /// boundary as a previous cursor may be re-served, but the receiver
    /// dedups so no duplication occurs in the row store.
    #[test]
    fn at_least_once_no_duplicates_at_receiver() {
        let project = test_project();
        let mut a = TxnNode::new(named_instance("A"));
        let mut b = TxnNode::new(named_instance("B"));

        // Open T1, insert v1.
        let t1 = a.begin();
        let v1 = a.insert(t1, create_args("res-1", &project, "s1", "v1"));

        // Open and commit T2.
        let t2 = a.begin();
        let _v2 = a.insert(t2, create_args("res-2", &project, "s2", "v2"));
        a.commit(t2);

        // B pulls — gets v2, cursor winds to t1 (since v2's watermark is t1).
        b.replicate_from(&a, &project);

        // T1 commits.
        a.commit(t1);

        // B pulls again: WHERE local_version >= t1, gets both v1 and v2.
        // v2 is re-served but dedup'd at the receiver.
        b.replicate_from(&a, &project);

        // No duplicates on B.
        assert_eq!(b.count_committed(), 2);
        assert!(b.has(&v1));
    }
}
