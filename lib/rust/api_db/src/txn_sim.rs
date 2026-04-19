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
    /// Replication path (writer to local instance).
    path: Vec<InstanceId>,
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

    /// One-shot replication: prepare a pull and apply it immediately.
    pub fn replicate_from(&mut self, other: &TxnNode, project_id: &ProjectId) -> usize {
        let batch = self.prepare_pull_from(other, project_id);
        self.apply_pull(batch)
    }

    /// Prepare a pull batch (no &mut self). Multiple in-flight pulls can be
    /// prepared concurrently and applied later in any order.
    pub fn prepare_pull_from(&self, other: &TxnNode, project_id: &ProjectId) -> PullBatch {
        let cursor_key = (other.instance_id.clone(), project_id.clone());
        let cursor = self.cursors.get(&cursor_key).copied().unwrap_or(0);
        let serve_embargoed = other.serves_embargo_to.contains(&self.instance_id);

        let to_send = other.visible_entries_for(project_id, cursor);
        let mut entries = Vec::new();
        let mut new_cursor = cursor;

        for row in to_send {
            // Take watermark even for filtered entries — the cursor advance
            // is independent of filtering decisions on the serve side.
            new_cursor = row.watermark;

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

        let existing = self.cursors.get(&cursor_key).copied().unwrap_or(0);
        self.cursors
            .insert(cursor_key, existing.max(batch.new_cursor));
        accepted
    }
}

/// A pull batch prepared by `prepare_pull_from`, ready to be applied.
#[derive(Debug, Clone)]
pub struct PullBatch {
    upstream: InstanceId,
    project_id: ProjectId,
    entries: Vec<ServedEntry>,
    new_cursor: u64,
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

        let v = a.auto_insert(create_args("r1", &project, "s1", ""));
        b.replicate_from(&a, &project);
        c.replicate_from(&b, &project);

        assert_eq!(a.path_for(&v).unwrap(), vec![a_id.clone()]);
        assert_eq!(b.path_for(&v).unwrap(), vec![a_id.clone(), b_id.clone()]);
        assert_eq!(c.path_for(&v).unwrap(), vec![a_id, b_id, c_id]);
    }

    // ── Combined: everything together ───────────────────────────────────

    /// All features simultaneously:
    /// - 4 nodes, 2 projects
    /// - Asymmetric embargo trust (A↔B trust each other; C, D do not)
    /// - Out-of-order commits on A while B/C/D pull
    /// - Concurrent pulls from multiple upstreams
    /// - Path propagation through multi-hop topology
    /// - Cross-project isolation maintained throughout
    ///
    /// After the workload, assert global invariants hold per project.
    #[test]
    fn kitchen_sink_scenario() {
        let p1 = test_project();
        let p2 = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let c_id = named_instance("C");
        let d_id = named_instance("D");

        let mut a = TxnNode::new(a_id.clone());
        let mut b = TxnNode::new(b_id.clone());
        let mut c = TxnNode::new(c_id.clone());
        let mut d = TxnNode::new(d_id.clone());

        // Asymmetric embargo trust.
        a.serve_embargo_to(&b_id);
        b.serve_embargo_to(&a_id);

        // ── Phase 1: writes on A with out-of-order commits ──
        // Open a long-running transaction T1 on A.
        let t1 = a.begin();
        let a_p1_secret = a.insert(
            t1,
            InsertArgs {
                resource_id: "secret-p1",
                project_id: &p1,
                slug: "secret",
                payload: "",
                embargoed: true,
                kind: JournalKind::Entry,
                previous_version: None,
            },
        );

        // Several quick auto-commit transactions on A in P1 and P2.
        let a_p1_pub = a.auto_insert(create_args("pub-p1", &p1, "pub", "x"));
        let a_p2_pub = a.auto_insert(create_args("pub-p2", &p2, "pub", "x"));

        // ── Phase 2: B and C pull P1 from A while T1 still open ──
        // Concurrent pulls (prepared at the same cursor).
        // T1 is still open, so a_p1_secret is not yet visible — neither B
        // nor C will see it yet, regardless of trust.
        let b_batch1 = b.prepare_pull_from(&a, &p1);
        let c_batch1 = c.prepare_pull_from(&a, &p1);
        b.apply_pull(b_batch1);
        c.apply_pull(c_batch1);

        // Both see the public entry, neither sees the secret yet.
        assert!(b.has(&a_p1_pub));
        assert!(!b.has(&a_p1_secret));
        assert!(c.has(&a_p1_pub));
        assert!(!c.has(&a_p1_secret));
        // Neither has a_p2_pub (different project).
        assert!(!b.has(&a_p2_pub));
        assert!(!c.has(&a_p2_pub));

        // ── Phase 3: T1 commits, then B and C re-pull. ──
        // Cursor on B/C is already past a_p1_pub but the watermark of
        // a_p1_pub was held back by T1's xmin, so the cursor wound back
        // and the next pull will re-serve a_p1_pub plus deliver a_p1_secret.
        // B (trusted) gets the secret; C (untrusted) does not.
        a.commit(t1);
        b.replicate_from(&a, &p1);
        c.replicate_from(&a, &p1);

        assert!(b.has(&a_p1_secret), "B is trusted, should now have secret");
        assert!(
            !c.has(&a_p1_secret),
            "C is not trusted, should still lack secret"
        );

        // ── Phase 4: D pulls P1 from B (relay). B has the secret because
        // A trusted B; B does NOT trust D, so D should not get the secret.
        // D should get a_p1_pub via B. ──
        d.replicate_from(&b, &p1);
        assert!(d.has(&a_p1_pub));
        assert!(!d.has(&a_p1_secret));
        // Path on D: [A, B, D].
        assert_eq!(
            d.path_for(&a_p1_pub).unwrap(),
            vec![a_id.clone(), b_id.clone(), d_id.clone()]
        );

        // ── Phase 5: Pull P2 on B and C/D — only a_p2_pub exists. ──
        b.replicate_from(&a, &p2);
        c.replicate_from(&a, &p2);
        d.replicate_from(&b, &p2);
        assert!(b.has(&a_p2_pub));
        assert!(c.has(&a_p2_pub));
        assert!(d.has(&a_p2_pub));

        // ── Phase 6: Concurrent pulls into D from both B (relay) and A
        // (direct). D already has a_p1_pub — second arrival dedups. ──
        let d_batch_from_b = d.prepare_pull_from(&b, &p1);
        let d_batch_from_a = d.prepare_pull_from(&a, &p1);
        let nb = d.apply_pull(d_batch_from_b);
        let na = d.apply_pull(d_batch_from_a);
        assert_eq!(nb + na, 0); // everything visible already deduped or not delivered

        // ── Phase 7: B does an edit on a replicated resource (cross-instance
        // edit), creating a new origin entry on B that references A's. ──
        let b_edit = b.auto_insert(InsertArgs {
            resource_id: "pub-p1",
            project_id: &p1,
            slug: "pub-edited",
            payload: "edited",
            embargoed: false,
            kind: JournalKind::Entry,
            previous_version: Some(a_p1_pub.clone()),
        });
        // Replicate B → C and B → D for P1.
        c.replicate_from(&b, &p1);
        d.replicate_from(&b, &p1);
        assert!(c.has(&b_edit));
        assert!(d.has(&b_edit));

        // ── Final invariant checks per project ──
        // P1: every node's previous_version chain is intact (or has a known
        // embargo gap on the secret for C/D).
        for node in [&a, &b, &c, &d] {
            for row in node.rows.values().filter(|r| r.committed) {
                if &row.entry.meta.project_id != &p1 {
                    continue;
                }
                let Some(prev) = &row.entry.header.previous_version else {
                    continue;
                };
                let prev_key = entry_key(prev);
                if !node.rows.contains_key(&prev_key) {
                    // Acceptable only if previous is the embargoed secret on
                    // a node that wasn't trusted.
                    assert_eq!(prev, &a_p1_secret);
                }
            }
        }

        // Each path starts with its origin and ends with the local node.
        for (node, _name) in [(&a, "A"), (&b, "B"), (&c, "C"), (&d, "D")] {
            for row in node.rows.values().filter(|r| r.committed) {
                assert_eq!(
                    row.path.first(),
                    Some(&row.entry.header.version.origin_instance_id),
                );
                assert_eq!(row.path.last(), Some(&node.instance_id));
            }
        }

        // Cross-project: no entry on a node has a different project_id than
        // the one its row was filed under (trivially true since row carries
        // the entry, but assert anyway).
        for node in [&a, &b, &c, &d] {
            for row in node.rows.values().filter(|r| r.committed) {
                assert!([&p1, &p2].contains(&&row.entry.meta.project_id));
            }
        }
    }
}
