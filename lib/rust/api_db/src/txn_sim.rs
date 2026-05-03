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

use crate::admin::UserId;
use crate::journal::{
    FederatedIdentity, FederatedVersion, InstanceId, JournalEntryHeader, JournalKind, LocalId,
    LocalTxnId, OriginId, ResourceEntryMeta, Slug,
};
use crate::replication_example::ReplicationExample;
use crate::role::ProjectId;

// ── Clock ────────────────────────────────────────────────────────────────

/// Models Postgres transaction ID allocation and snapshot xmin computation.
#[derive(Debug, Default)]
pub struct Clock {
    next_txid: u64,
    in_flight: BTreeSet<LocalTxnId>,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            next_txid: LocalTxnId::MIN,
            in_flight: BTreeSet::new(),
        }
    }

    /// Allocate a new txid and mark it in-flight.
    /// Equivalent to `pg_current_xact_id()` at the start of a transaction.
    pub fn begin(&mut self) -> LocalTxnId {
        let txid = LocalTxnId::new(self.next_txid).unwrap();
        self.next_txid += 1;
        self.in_flight.insert(txid);
        txid
    }

    /// Return the lowest still-running txid.
    /// Equivalent to `pg_snapshot_xmin(pg_current_snapshot())`.
    /// If no transactions are in flight, returns `next_txid` (everything before
    /// is committed; nothing equal or greater exists yet).
    pub fn xmin(&self) -> LocalTxnId {
        self.in_flight
            .first()
            .copied()
            .unwrap_or_else(|| LocalTxnId::new(self.next_txid).unwrap())
    }

    /// Mark a txid as committed (no longer in flight).
    pub fn commit(&mut self, txid: LocalTxnId) {
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
    local_version: LocalTxnId,
    /// xmin observed at INSERT time (snapshot of in-flight set).
    watermark: LocalTxnId,
    /// Whether the writing transaction has committed (and thus the row is
    /// visible to other readers).
    committed: bool,
    /// Replication path (writer to local instance).
    path: Vec<InstanceId>,
}

type EntryKey = (InstanceId, OriginId, u64);

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
    pending_by_txid: HashMap<LocalTxnId, Vec<EntryKey>>,
    /// Replication cursor per (upstream, project).  Absence = never pulled.
    cursors: HashMap<(InstanceId, ProjectId), LocalTxnId>,
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
    pub fn begin(&mut self) -> LocalTxnId {
        self.clock.begin()
    }

    /// Insert a row inside an open transaction.
    /// `local_version` = the transaction's txid.
    /// `watermark` = xmin at this moment (captured by the DEFAULT in real PG).
    pub fn insert(&mut self, txid: LocalTxnId, args: InsertArgs) -> FederatedVersion {
        assert!(
            self.pending_by_txid.contains_key(&txid) || self.clock.in_flight.contains(&txid),
            "txid {} is not an open transaction",
            txid.get(),
        );

        let watermark = self.clock.xmin();

        let version = FederatedVersion {
            origin_instance_id: self.instance_id.clone(),
            origin_id: args.origin_id,
            version: NonZeroU64::new(txid.get()).unwrap(),
        };
        let header = JournalEntryHeader {
            kind: args.kind,
            at: chrono::Utc::now(),
            author: FederatedIdentity {
                instance_id: self.instance_id.clone(),
                local_id: LocalId::User(UserId::new()),
            },
            version: version.clone(),
            previous_version: args.previous_version,
            embargoed: args.embargoed,
        };
        let meta = ResourceEntryMeta {
            slug: Slug::new(args.slug).expect("sim: invalid slug"),
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
                path: vec![self.instance_id.clone()],
            },
        );
        self.pending_by_txid.entry(txid).or_default().push(key);
        version
    }

    /// Replication path for a stored entry, if present.
    pub fn path_for(&self, version: &FederatedVersion) -> Option<Vec<InstanceId>> {
        self.rows
            .get(&entry_key(version))
            .filter(|r| r.committed)
            .map(|r| r.path.clone())
    }

    /// All entries on this node in a project (for invariant checks).
    pub fn entries_in_project(&self, project_id: &ProjectId) -> Vec<&ReplicationExample> {
        self.rows
            .values()
            .filter(|r| r.committed)
            .filter(|r| &r.entry.meta.project_id == project_id)
            .map(|r| &r.entry)
            .collect()
    }

    /// Commit a transaction: make all its inserts visible.
    pub fn commit(&mut self, txid: LocalTxnId) {
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
    /// `cursor == None` matches every row (fresh pull).
    fn visible_entries_for(
        &self,
        project_id: &ProjectId,
        cursor: Option<LocalTxnId>,
    ) -> Vec<&StoredRow> {
        let mut rows: Vec<&StoredRow> = self
            .rows
            .values()
            .filter(|r| r.committed)
            .filter(|r| &r.entry.meta.project_id == project_id)
            .filter(|r| cursor.is_none_or(|c| r.local_version >= c))
            .collect();
        rows.sort_by_key(|r| r.local_version);
        rows
    }

    /// One-shot replication: prepare a pull and apply it immediately.
    pub fn replicate_from(&mut self, other: &TxnNode, project_id: &ProjectId) -> usize {
        let batch = self.prepare_pull_from(other, project_id);
        self.apply_pull(batch)
    }

    /// Prepare a pull batch (no &mut self). Multiple in-flight pulls can be
    /// prepared concurrently and applied later in any order.
    pub fn prepare_pull_from(&self, other: &TxnNode, project_id: &ProjectId) -> PullBatch {
        let cursor_key = (other.instance_id.clone(), project_id.clone());
        let cursor = self.cursors.get(&cursor_key).copied();

        let serve_embargoed = other.serves_embargo_to.contains(&self.instance_id);

        let to_send = other.visible_entries_for(project_id, cursor);
        let mut entries = Vec::new();
        let mut new_cursor = cursor;

        for row in to_send {
            // Take watermark even for filtered entries — the cursor advance
            // is independent of filtering decisions on the serve side.
            new_cursor = Some(row.watermark);

            if row.entry.header.version.origin_instance_id == self.instance_id {
                continue;
            }
            if row.entry.header.embargoed && !serve_embargoed {
                continue;
            }
            entries.push(ServedEntry {
                entry: row.entry.clone(),
                source_path: row.path.clone(),
            });
        }

        PullBatch {
            upstream: other.instance_id.clone(),
            project_id: project_id.clone(),
            entries,
            new_cursor,
        }
    }

    /// Apply a previously-prepared batch. Dedups, propagates path, advances
    /// cursor monotonically (out-of-order applies don't regress).
    pub fn apply_pull(&mut self, batch: PullBatch) -> usize {
        let cursor_key = (batch.upstream.clone(), batch.project_id.clone());
        let mut accepted = 0;

        for served in batch.entries {
            let key = entry_key(&served.entry.header.version);
            if self.rows.contains_key(&key) {
                continue;
            }
            // Receive in its own auto-commit transaction.
            let receive_txid = self.clock.begin();
            let mut new_path = served.source_path;
            new_path.push(self.instance_id.clone());
            self.rows.insert(
                key.clone(),
                StoredRow {
                    entry: served.entry,
                    local_version: receive_txid,
                    watermark: self.clock.xmin(),
                    committed: false,
                    path: new_path,
                },
            );
            self.clock.commit(receive_txid);
            self.rows.get_mut(&key).unwrap().committed = true;
            accepted += 1;
        }

        // Monotone cursor advance — out-of-order applies don't regress it.
        let existing = self.cursors.get(&cursor_key).copied();
        let new_cursor = match (existing, batch.new_cursor) {
            (None, b) => b,
            (Some(e), None) => Some(e),
            (Some(e), Some(b)) => Some(e.max(b)),
        };
        if let Some(c) = new_cursor {
            self.cursors.insert(cursor_key, c);
        }
        accepted
    }
}

/// A pull batch prepared by `prepare_pull_from`, ready to be applied.
#[derive(Debug, Clone)]
pub struct PullBatch {
    upstream: InstanceId,
    project_id: ProjectId,
    entries: Vec<ServedEntry>,
    new_cursor: Option<LocalTxnId>,
}

impl PullBatch {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone)]
struct ServedEntry {
    entry: ReplicationExample,
    source_path: Vec<InstanceId>,
}

// ── Argument structs ─────────────────────────────────────────────────────

pub struct InsertArgs<'a> {
    pub origin_id: OriginId,
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
        project_id: &'a ProjectId,
        slug: &'a str,
        payload: &'a str,
    ) -> InsertArgs<'a> {
        InsertArgs {
            origin_id: OriginId::new(),
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
        // No txns in flight, xmin returns next_txid.
        // Started at MIN=3, allocated t1 and t2, so next_txid is 5.
        assert_eq!(c.xmin().get(), LocalTxnId::MIN + 2);
    }

    // ── Out-of-order commit semantics ───────────────────────────────────

    #[test]
    fn watermark_lags_under_long_running_txn() {
        let project = test_project();
        let mut a = TxnNode::new(named_instance("A"));

        // Open a long-running transaction T1.
        let t1 = a.begin();
        let v1 = a.insert(t1, create_args(&project, "s1", "from-t1"));

        // Many quick transactions commit while T1 is open.
        let mut later = Vec::new();
        for i in 0..5 {
            later.push(a.auto_insert(create_args(&project, &format!("s{}", i + 2), "p")));
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
        let v1 = a.insert(t1, create_args(&project, "s1", "t1-row"));

        // Open and commit T2 (txid=2) on A.
        let t2 = a.begin();
        let v2 = a.insert(t2, create_args(&project, "s2", "t2-row"));
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
        let v1 = a.insert(t1, create_args(&project, "s1", "v1"));
        let txns: Vec<(LocalTxnId, FederatedVersion)> = (2..=5)
            .map(|i| {
                let txid = a.begin();
                let v = a.insert(
                    txid,
                    create_args(&project, &format!("s{i}"), &format!("v{i}")),
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

        let _ = a.auto_insert(create_args(&project, "s1", "p"));
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
        let v1 = a.insert(t1, create_args(&project, "s1", "v1"));

        // Open and commit T2.
        let t2 = a.begin();
        let _v2 = a.insert(t2, create_args(&project, "s2", "v2"));
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

    // ── Path tracking on TxnNode ────────────────────────────────────────

    #[test]
    fn path_propagates_through_chain() {
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let c_id = named_instance("C");
        let mut a = TxnNode::new(a_id.clone());
        let mut b = TxnNode::new(b_id.clone());
        let mut c = TxnNode::new(c_id.clone());

        let v = a.auto_insert(create_args(&project, "s1", ""));
        b.replicate_from(&a, &project);
        c.replicate_from(&b, &project);

        assert_eq!(a.path_for(&v).unwrap(), vec![a_id.clone()]);
        assert_eq!(b.path_for(&v).unwrap(), vec![a_id.clone(), b_id.clone()]);
        assert_eq!(c.path_for(&v).unwrap(), vec![a_id, b_id, c_id]);
    }

    // ── Integration: features interacting ───────────────────────────────
    //
    // One focused test per compound invariant.  Each holds a specific
    // interaction between: out-of-order commits, embargo trust, multi-hop
    // replication, cross-project isolation, and cross-instance edits.
    // A failure points at a single interaction, not a whole 7-phase script.

    /// A trusts B for embargo, A does not trust C.  A writes an embargoed
    /// row inside a long-running transaction, then commits a public row
    /// while the long txn is still open.  B and C pull concurrently.
    ///
    /// Invariant: a trusted peer receives the embargoed row after the long
    /// transaction commits (the watermark winds back past the in-flight
    /// txid), while an untrusted peer never receives it.
    #[test]
    fn out_of_order_commit_interacts_with_asymmetric_embargo() {
        let project = test_project();
        let (a_id, b_id, c_id) = (
            named_instance("A"),
            named_instance("B"),
            named_instance("C"),
        );
        let (mut a, mut b, mut c) = (
            TxnNode::new(a_id.clone()),
            TxnNode::new(b_id.clone()),
            TxnNode::new(c_id.clone()),
        );
        a.serve_embargo_to(&b_id); // A→B only

        // Long-running txn holding the embargoed row uncommitted.
        let t_secret = a.begin();
        let secret = a.insert(
            t_secret,
            InsertArgs {
                origin_id: OriginId::new(),
                project_id: &project,
                slug: "secret",
                payload: "",
                embargoed: true,
                kind: JournalKind::Entry,
                previous_version: None,
            },
        );
        let public = a.auto_insert(create_args(&project, "public", ""));

        // Pulls prepared while secret is still in-flight.
        b.apply_pull(b.prepare_pull_from(&a, &project));
        c.apply_pull(c.prepare_pull_from(&a, &project));
        assert!(b.has(&public) && c.has(&public));
        assert!(!b.has(&secret) && !c.has(&secret), "secret still in-flight");

        // Long txn commits; re-pull.  Watermark winds back, re-serves.
        a.commit(t_secret);
        b.replicate_from(&a, &project);
        c.replicate_from(&a, &project);
        assert!(b.has(&secret), "trusted peer receives secret post-commit");
        assert!(!c.has(&secret), "untrusted peer never receives secret");
    }

    /// Three-hop replication (A→B→D) with an asymmetric trust path where
    /// the relay (B) holds a secret the final hop (D) must not receive.
    ///
    /// Invariant: the `path` field reflects the actual replication route,
    /// and embargo filtering applies per hop (B serves to D only what B
    /// is configured to serve, regardless of what B itself received).
    #[test]
    fn multi_hop_path_respects_per_hop_embargo() {
        let project = test_project();
        let (a_id, b_id, d_id) = (
            named_instance("A"),
            named_instance("B"),
            named_instance("D"),
        );
        let (mut a, mut b, mut d) = (
            TxnNode::new(a_id.clone()),
            TxnNode::new(b_id.clone()),
            TxnNode::new(d_id.clone()),
        );
        a.serve_embargo_to(&b_id); // A trusts B; B does NOT trust D.

        let secret = a.auto_insert(InsertArgs {
            origin_id: OriginId::new(),
            project_id: &project,
            slug: "secret",
            payload: "",
            embargoed: true,
            kind: JournalKind::Entry,
            previous_version: None,
        });
        let public = a.auto_insert(create_args(&project, "public", ""));

        b.replicate_from(&a, &project);
        d.replicate_from(&b, &project);

        assert!(b.has(&secret), "relay has secret via trust from A");
        assert!(
            !d.has(&secret),
            "final hop lacks secret: relay doesn't trust it"
        );
        assert_eq!(
            d.path_for(&public).unwrap(),
            vec![a_id, b_id, d_id],
            "path records every hop, not just origin+local",
        );
    }

    /// Two projects share one writer.  Cursors are per-(upstream,project),
    /// so pulling project P1 must not affect the reader's P2 cursor.
    /// A cross-instance edit in one project must also not spill into the
    /// other.
    ///
    /// Invariant: project boundary is a hard partition in the replication
    /// stream — cursor progress, entry delivery, and `previous_version`
    /// chains are all scoped to one project.
    #[test]
    fn cross_project_partition_holds_under_cross_instance_edit() {
        let (p1, p2) = (test_project(), test_project());
        let (a_id, b_id) = (named_instance("A"), named_instance("B"));
        let (mut a, mut b) = (TxnNode::new(a_id.clone()), TxnNode::new(b_id.clone()));

        let a_p1 = a.auto_insert(create_args(&p1, "a1", ""));
        let _a_p2 = a.auto_insert(create_args(&p2, "a1", ""));

        // Pull only P1.  B should know P1's cursor, not P2's.
        b.replicate_from(&a, &p1);
        assert!(b.has(&a_p1));

        // B edits the P1 resource it just received.
        let b_edit = b.auto_insert(InsertArgs {
            origin_id: a_p1.origin_id.clone(),
            project_id: &p1,
            slug: "a1-edited",
            payload: "edited",
            embargoed: false,
            kind: JournalKind::Entry,
            previous_version: Some(a_p1.clone()),
        });
        assert_eq!(b_edit.origin_instance_id, b_id, "edit originates on B");
        assert_eq!(
            b_edit.origin_id, a_p1.origin_id,
            "edit preserves the resource's origin_id across the rename/edit",
        );

        // Pulling P2 now still delivers A's P2 row (cursor wasn't advanced
        // by the P1 pull).
        let n = b.replicate_from(&a, &p2);
        assert_eq!(n, 1, "P2 cursor is independent of P1");

        // The P1 edit replicates back to A without touching P2.
        let a_p2_count_before = a.entries_in_project(&p2).len();
        a.replicate_from(&b, &p1);
        assert!(a.has(&b_edit));
        assert_eq!(
            a.entries_in_project(&p2).len(),
            a_p2_count_before,
            "P1 replication did not spill into P2",
        );
    }
}
