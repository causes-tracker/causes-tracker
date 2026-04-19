//! Multi-instance replication simulator for property testing.
//!
//! Scaffolding — not part of the production API. Explores the replication
//! protocol's logical behavior across a graph of simulated instances,
//! without requiring multiple Postgres databases.
//!
//! Covers: dedup, embargo filtering (including un-embargo), topological
//! ordering, rename via slug change, multi-hop delivery, at-least-once
//! redelivery, and replication path tracking.
//!
//! Simplifying assumptions vs. the real protocol:
//! - Writes on each node are serialized: `local_version == watermark`.
//!   Out-of-order commits (where watermark < local_version) are modeled in
//!   the separate `txn_sim` module.
//! - Embargo trust is per directed pair (asymmetric supported).

#![allow(dead_code)] // scaffolding for tests

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;

use sqlx::types::chrono;
use uuid::Uuid;

use crate::admin::UserId;
use crate::journal::{
    FederatedIdentity, FederatedVersion, InstanceId, JournalEntryHeader, JournalKind, LocalId,
    LocalTxnId, OriginId, ResourceEntryMeta, Slug,
};
use crate::replication_example::ReplicationExample;
use crate::role::ProjectId;

// ── Test helpers ─────────────────────────────────────────────────────────

/// Unique instance_id for a named node in tests.
pub fn named_instance(_name: &str) -> InstanceId {
    // New v4 — names are only for readability of failing test output,
    // identity uses real UUIDs.
    InstanceId::from_raw(&Uuid::new_v4().to_string()).unwrap()
}

pub fn test_project() -> ProjectId {
    ProjectId::new(Uuid::new_v4().to_string()).unwrap()
}

// ── SimStoredEntry ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SimStoredEntry {
    entry: ReplicationExample,
    /// Position in this node's commit order — assigned when the node writes
    /// or accepts the entry.
    local_version: LocalTxnId,
    /// Safe resume point for replication. Simplified: equal to local_version.
    watermark: LocalTxnId,
    /// Replication path: ordered list of instance_ids, from the writing
    /// instance to this storing instance. Locally-authored: [self].
    /// Replicated: [...source path, self].
    path: Vec<InstanceId>,
}

// ── SimNode ──────────────────────────────────────────────────────────────

/// In-memory simulation of a Causes instance.
#[derive(Debug)]
pub struct SimNode {
    instance_id: InstanceId,
    /// All entries stored on this node, keyed by federated identity triple.
    entries: HashMap<EntryKey, SimStoredEntry>,
    /// Next local_version to assign.  Starts at `LocalTxnId::MIN`;
    /// assigned-then-incremented.
    next_local_version: u64,
    /// Replication cursor per (upstream, project).  Absence = never pulled.
    cursors: HashMap<(InstanceId, ProjectId), LocalTxnId>,
    /// Peers this node will serve embargoed entries to (outbound trust).
    serves_embargo_to: HashSet<InstanceId>,
}

type EntryKey = (InstanceId, OriginId, u64);

fn entry_key(v: &FederatedVersion) -> EntryKey {
    (
        v.origin_instance_id.clone(),
        v.origin_id.clone(),
        v.version.get(),
    )
}

impl SimNode {
    pub fn new(instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            entries: HashMap::new(),
            next_local_version: LocalTxnId::MIN,
            cursors: HashMap::new(),
            serves_embargo_to: HashSet::new(),
        }
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    /// Configure this node to serve embargoed entries to `peer`.
    pub fn serve_embargo_to(&mut self, peer: &InstanceId) {
        self.serves_embargo_to.insert(peer.clone());
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn has(&self, version: &FederatedVersion) -> bool {
        self.entries.contains_key(&entry_key(version))
    }

    pub fn entries_in_project(&self, project_id: &ProjectId) -> Vec<&ReplicationExample> {
        self.entries
            .values()
            .filter(|se| &se.entry.meta.project_id == project_id)
            .map(|se| &se.entry)
            .collect()
    }

    /// Iterate over all entries' federated versions on this node.
    pub fn all_versions(&self) -> Vec<FederatedVersion> {
        self.entries
            .values()
            .map(|se| se.entry.header.version.clone())
            .collect()
    }

    // ── Local write operations ──────────────────────────────────────────

    /// Create a new resource on this node.
    pub fn create(&mut self, args: CreateArgs) -> FederatedVersion {
        self.write(WriteArgs {
            origin_id: OriginId::new(),
            project_id: args.project_id,
            slug: args.slug,
            payload: args.payload,
            embargoed: args.embargoed,
            kind: JournalKind::Entry,
            previous_version: None,
        })
    }

    /// Edit an existing resource on this node.
    pub fn edit(&mut self, args: EditArgs) -> FederatedVersion {
        let origin_id = args.previous.origin_id.clone();
        self.write(WriteArgs {
            origin_id,
            project_id: args.project_id,
            slug: args.slug,
            payload: args.payload,
            embargoed: args.embargoed,
            kind: JournalKind::Entry,
            previous_version: Some(args.previous),
        })
    }

    /// Rename a resource: edit with a new slug, same origin_id.
    pub fn rename(&mut self, args: RenameArgs) -> FederatedVersion {
        let origin_id = args.previous.origin_id.clone();
        self.write(WriteArgs {
            origin_id,
            project_id: args.project_id,
            slug: args.new_slug,
            payload: args.payload,
            embargoed: args.embargoed,
            kind: JournalKind::Entry,
            previous_version: Some(args.previous),
        })
    }

    /// Tombstone a resource.
    pub fn delete(&mut self, args: DeleteArgs) -> FederatedVersion {
        let origin_id = args.previous.origin_id.clone();
        self.write(WriteArgs {
            origin_id,
            project_id: args.project_id,
            slug: args.slug,
            payload: "",
            embargoed: args.embargoed,
            kind: JournalKind::Tombstone,
            previous_version: Some(args.previous),
        })
    }

    fn write(&mut self, args: WriteArgs) -> FederatedVersion {
        let local_version = LocalTxnId::new(self.next_local_version).unwrap();
        self.next_local_version += 1;

        let version = FederatedVersion {
            origin_instance_id: self.instance_id.clone(),
            origin_id: args.origin_id,
            version: NonZeroU64::new(local_version.get()).unwrap(),
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

        self.entries.insert(
            entry_key(&version),
            SimStoredEntry {
                entry,
                local_version,
                watermark: local_version,
                path: vec![self.instance_id.clone()],
            },
        );
        version
    }

    /// Get the replication path for a stored entry.
    pub fn path_for(&self, version: &FederatedVersion) -> Option<Vec<InstanceId>> {
        self.entries
            .get(&entry_key(version))
            .map(|se| se.path.clone())
    }

    // ── Replication ─────────────────────────────────────────────────────

    /// Pull new entries from `other` for a given project, applying dedup
    /// and embargo filtering. Returns the number of new entries accepted.
    pub fn replicate_from(&mut self, other: &SimNode, project_id: &ProjectId) -> usize {
        let batch = self.prepare_pull_from(other, project_id);
        self.apply_pull(batch)
    }

    /// Prepare a pull batch from `other` based on this node's current cursor.
    /// Does not modify `self` — multiple in-flight pulls can be prepared
    /// concurrently and applied in any order.
    pub fn prepare_pull_from(&self, other: &SimNode, project_id: &ProjectId) -> PullBatch {
        let cursor_key = (other.instance_id.clone(), project_id.clone());
        let cursor = self.cursors.get(&cursor_key).copied();

        let serve_embargoed = other.serves_embargo_to.contains(&self.instance_id);

        let mut to_send: Vec<&SimStoredEntry> = other
            .entries
            .values()
            .filter(|se| &se.entry.meta.project_id == project_id)
            .filter(|se| cursor.is_none_or(|c| se.local_version > c))
            .collect();
        to_send.sort_by_key(|se| se.local_version);

        let mut served = Vec::with_capacity(to_send.len());
        let mut max_watermark = cursor;
        for se in to_send {
            // Always advance max_watermark — filtered entries still
            // contribute to the cursor.
            if max_watermark.is_none_or(|m| se.watermark > m) {
                max_watermark = Some(se.watermark);
            }
            // Apply embargo and origin filters at serve time.
            if se.entry.header.version.origin_instance_id == self.instance_id {
                continue;
            }
            if se.entry.header.embargoed && !serve_embargoed {
                continue;
            }
            served.push(ServedEntry {
                entry: se.entry.clone(),
                source_path: se.path.clone(),
            });
        }

        PullBatch {
            upstream: other.instance_id.clone(),
            project_id: project_id.clone(),
            entries: served,
            new_cursor: max_watermark,
        }
    }

    /// Apply a previously-prepared pull batch.
    /// Dedups entries that this node already has. Returns count of new entries.
    /// Updates the cursor to `max(existing, batch.new_cursor)` — this means
    /// out-of-order applies of two batches still leave the cursor at the
    /// higher value (no progress lost).
    pub fn apply_pull(&mut self, batch: PullBatch) -> usize {
        let cursor_key = (batch.upstream.clone(), batch.project_id.clone());
        let mut accepted = 0;

        for served in batch.entries {
            let key = entry_key(&served.entry.header.version);
            if self.entries.contains_key(&key) {
                continue;
            }
            let our_local_version = LocalTxnId::new(self.next_local_version).unwrap();
            self.next_local_version += 1;
            let mut new_path = served.source_path;
            new_path.push(self.instance_id.clone());
            self.entries.insert(
                key,
                SimStoredEntry {
                    entry: served.entry,
                    local_version: our_local_version,
                    watermark: our_local_version,
                    path: new_path,
                },
            );
            accepted += 1;
        }

        // Take the max so out-of-order applies don't regress the cursor.
        // `None` cursor means "never pulled" — any cursor from the batch wins.
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

/// A batch of entries served by a `prepare_pull_from`, ready to be applied.
#[derive(Debug, Clone)]
pub struct PullBatch {
    upstream: InstanceId,
    project_id: ProjectId,
    entries: Vec<ServedEntry>,
    /// Watermark of the last entry served (cursor to store after apply).
    /// `None` = the batch is empty, no cursor change needed.
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

// ── Write operation argument structs ─────────────────────────────────────

pub struct CreateArgs<'a> {
    pub project_id: &'a ProjectId,
    pub slug: &'a str,
    pub payload: &'a str,
    pub embargoed: bool,
}

pub struct EditArgs<'a> {
    pub previous: FederatedVersion,
    pub project_id: &'a ProjectId,
    pub slug: &'a str,
    pub payload: &'a str,
    pub embargoed: bool,
}

pub struct RenameArgs<'a> {
    pub previous: FederatedVersion,
    pub project_id: &'a ProjectId,
    pub new_slug: &'a str,
    pub payload: &'a str,
    pub embargoed: bool,
}

pub struct DeleteArgs<'a> {
    pub previous: FederatedVersion,
    pub project_id: &'a ProjectId,
    pub slug: &'a str,
    pub embargoed: bool,
}

struct WriteArgs<'a> {
    origin_id: OriginId,
    project_id: &'a ProjectId,
    slug: &'a str,
    payload: &'a str,
    embargoed: bool,
    kind: JournalKind,
    previous_version: Option<FederatedVersion>,
}

// ── Topology helpers ─────────────────────────────────────────────────────

/// A directed edge (from, to) in the replication topology.
pub type Edge = (usize, usize);

/// Split-borrow helper: get two mutable references to distinct slice elements.
fn get_two_mut<T>(slice: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert_ne!(i, j);
    if i < j {
        let (left, right) = slice.split_at_mut(j);
        (&mut left[i], &mut right[0])
    } else {
        let (left, right) = slice.split_at_mut(i);
        (&mut right[0], &mut left[j])
    }
}

/// Run one round of replication over the given edges.
/// Returns total new entries accepted across all edges.
pub fn replicate_round(nodes: &mut [SimNode], edges: &[Edge], project_id: &ProjectId) -> usize {
    let mut total = 0;
    for &(from, to) in edges {
        let (from_n, to_n) = get_two_mut(nodes, from, to);
        total += to_n.replicate_from(from_n, project_id);
    }
    total
}

/// Run replication rounds until no new entries are accepted anywhere
/// or `max_rounds` is exhausted.
/// Panics if not quiescent after max_rounds.
pub fn replicate_until_quiescent(
    nodes: &mut [SimNode],
    edges: &[Edge],
    project_id: &ProjectId,
    max_rounds: usize,
) -> usize {
    let mut rounds = 0;
    loop {
        let accepted = replicate_round(nodes, edges, project_id);
        rounds += 1;
        if accepted == 0 {
            return rounds;
        }
        if rounds >= max_rounds {
            panic!("replication did not reach quiescence after {max_rounds} rounds");
        }
    }
}

// ── Invariant checks ─────────────────────────────────────────────────────

/// Assert that within each node, no two entries share a federated version.
/// (Trivially true given BTreeMap keying, but asserted to document the invariant.)
pub fn assert_dedup_per_node(nodes: &[SimNode]) {
    for node in nodes {
        let keys: HashSet<EntryKey> = node.entries.keys().cloned().collect();
        assert_eq!(
            keys.len(),
            node.entries.len(),
            "node {} has duplicates",
            node.instance_id
        );
    }
}

/// Assert that for every entry on every node, if it has a `previous_version`
/// reference, either:
/// - the previous entry is present on the same node, OR
/// - the previous entry is embargoed (acceptable gap).
pub fn assert_previous_version_or_embargo_gap(
    nodes: &[SimNode],
    all_entries_by_key: &HashMap<EntryKey, bool>,
) {
    for node in nodes {
        for se in node.entries.values() {
            let Some(prev) = &se.entry.header.previous_version else {
                continue;
            };
            let prev_key = entry_key(prev);
            if node.entries.contains_key(&prev_key) {
                continue;
            }
            // Gap is only acceptable if the referenced entry exists globally
            // AND is embargoed.
            let prev_embargoed = all_entries_by_key.get(&prev_key).copied();
            match prev_embargoed {
                Some(true) => {} // embargo gap is expected
                Some(false) => panic!(
                    "node {} missing non-embargoed previous_version {:?} of entry {:?}",
                    node.instance_id, prev_key, se.entry.header.version,
                ),
                None => panic!(
                    "node {} references previous_version {:?} that does not exist anywhere",
                    node.instance_id, prev_key
                ),
            }
        }
    }
}

/// Build a map of every entry keyed by federated identity, to its embargoed flag.
/// Used for invariant checks.
pub fn global_entry_embargo_map(nodes: &[SimNode]) -> HashMap<EntryKey, bool> {
    let mut map = HashMap::new();
    for node in nodes {
        for se in node.entries.values() {
            map.insert(
                entry_key(&se.entry.header.version),
                se.entry.header.embargoed,
            );
        }
    }
    map
}

/// Assert that after full bidirectional replication, every non-embargoed
/// entry on any origin is present on every node, unless reachability through
/// the topology would block it.
///
/// Simplification: we check an all-pairs reachability. For each node N, every
/// entry E originated anywhere should be on N unless:
/// - E is embargoed AND no path from E.origin to N has embargo trust all the
///   way through, OR
/// - N is the origin (trivially has it).
///
/// This helper assumes fully-connected topology (edges in both directions
/// between every pair). Tests with partial topology should use bespoke checks.
pub fn assert_all_non_embargoed_everywhere_in_mesh(nodes: &[SimNode], project_id: &ProjectId) {
    let all = global_entry_embargo_map(nodes);
    for node in nodes {
        for (key, embargoed) in &all {
            if *embargoed {
                continue;
            }
            // Only check entries in the project we replicate.
            let entry_in_project = nodes.iter().flat_map(|n| n.entries.values()).any(|se| {
                entry_key(&se.entry.header.version) == *key
                    && &se.entry.meta.project_id == project_id
            });
            if !entry_in_project {
                continue;
            }
            assert!(
                node.entries.contains_key(key),
                "node {} missing non-embargoed entry {:?}",
                node.instance_id,
                key
            );
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;
    use rand::{Rng, SeedableRng};

    fn mesh_edges(n: usize) -> Vec<Edge> {
        let mut edges = Vec::new();
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    edges.push((i, j));
                }
            }
        }
        edges
    }

    // ── Scenario tests ──────────────────────────────────────────────────

    #[test]
    fn single_hop_a_to_b() {
        let project = test_project();
        let a = named_instance("A");
        let b = named_instance("B");
        let mut nodes = vec![SimNode::new(a), SimNode::new(b)];

        nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "v1",
            embargoed: false,
        });

        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);

        assert_eq!(nodes[0].count(), 1);
        assert_eq!(nodes[1].count(), 1);
        assert_dedup_per_node(&nodes);
    }

    #[test]
    fn chain_a_b_c() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
            SimNode::new(named_instance("C")),
        ];
        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "v1",
            embargoed: false,
        });

        // Round 1: A→B
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        assert!(nodes[1].has(&v1));
        // Round 2: B→C
        replicate_until_quiescent(&mut nodes, &[(1, 2)], &project, 4);
        assert!(nodes[2].has(&v1));

        // Now B edits the resource.
        let v2 = nodes[1].edit(EditArgs {
            previous: v1.clone(),
            project_id: &project,
            slug: "foo",
            payload: "v2-from-b",
            embargoed: false,
        });

        // v2 originates at B (with origin_id = "res-1" but origin_instance_id = B).
        assert_eq!(v2.origin_instance_id, *nodes[1].instance_id());

        // Replicate all edges — C should get v2.
        replicate_until_quiescent(&mut nodes, &mesh_edges(3), &project, 10);

        assert!(nodes[2].has(&v2));
        assert!(nodes[0].has(&v2));
    }

    #[test]
    fn bidirectional_a_b_no_duplicates_after_roundtrip() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];

        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "v1",
            embargoed: false,
        });

        // Many rounds — should converge and stay stable.
        for _ in 0..5 {
            replicate_round(&mut nodes, &[(0, 1), (1, 0)], &project);
        }

        assert_eq!(nodes[0].count(), 1);
        assert_eq!(nodes[1].count(), 1);
        assert!(nodes[0].has(&v1));
        assert!(nodes[1].has(&v1));
    }

    #[test]
    fn embargo_not_delivered_to_untrusted() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];

        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "secret",
            embargoed: true,
        });

        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);

        assert!(nodes[0].has(&v1));
        assert!(!nodes[1].has(&v1));
        // Cursor should still have advanced despite filtering.
        // A subsequent non-embargoed entry should still be received.
        let v2 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "bar",
            payload: "open",
            embargoed: false,
        });
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        assert!(nodes[1].has(&v2));
        assert!(!nodes[1].has(&v1));
    }

    #[test]
    fn embargo_delivered_to_trusted() {
        let project = test_project();
        let a = named_instance("A");
        let b = named_instance("B");
        let mut nodes = vec![SimNode::new(a.clone()), SimNode::new(b.clone())];
        nodes[0].serve_embargo_to(&b);

        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "secret",
            embargoed: true,
        });

        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        assert!(nodes[1].has(&v1));
    }

    #[test]
    fn unembargo_flows_to_untrusted() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];

        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "secret",
            embargoed: true,
        });
        // B does not get v1 — embargoed and not trusted.
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        assert!(!nodes[1].has(&v1));

        // A un-embargoes by writing a new entry referencing v1, with embargoed = false.
        let v2 = nodes[0].edit(EditArgs {
            previous: v1.clone(),
            project_id: &project,
            slug: "foo",
            payload: "disclosed",
            embargoed: false,
        });

        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        assert!(nodes[1].has(&v2));
        // B's previous_version chain has a gap (v1 is embargoed, not delivered).
        let stored_v2 = nodes[1].entries.get(&entry_key(&v2)).unwrap();
        let prev = stored_v2.entry.header.previous_version.as_ref().unwrap();
        assert_eq!(prev, &v1);
        assert!(!nodes[1].has(prev));

        // Invariant checker must accept this gap since v1 is embargoed globally.
        let embargo_map = global_entry_embargo_map(&nodes);
        assert_previous_version_or_embargo_gap(&nodes, &embargo_map);
    }

    // ── Asymmetric embargo trust ────────────────────────────────────────

    /// A trusts B with embargo, but B does not trust A.
    /// Embargoed entries flow A→B but not B→A.
    #[test]
    fn embargo_trust_is_asymmetric() {
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let mut nodes = vec![SimNode::new(a_id.clone()), SimNode::new(b_id.clone())];

        // Asymmetric: A serves embargo to B, B does NOT serve to A.
        nodes[0].serve_embargo_to(&b_id);

        let secret_a = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "sa",
            payload: "from-A",
            embargoed: true,
        });
        let secret_b = nodes[1].create(CreateArgs {
            project_id: &project,
            slug: "sb",
            payload: "from-B",
            embargoed: true,
        });

        // Bidirectional replication.
        for _ in 0..3 {
            replicate_round(&mut nodes, &[(0, 1), (1, 0)], &project);
        }

        // B has A's secret (A serves embargo to B).
        assert!(nodes[1].has(&secret_a));
        // A does NOT have B's secret (B does not serve embargo to A).
        assert!(!nodes[0].has(&secret_b));
    }

    /// Three-node case: A trusts B; B trusts C; A does NOT trust C.
    /// An A-originated embargoed entry flows A→B→C only if B chooses to
    /// re-serve it to C (i.e. B trusts C). Verify B is the gateway.
    #[test]
    fn embargo_relay_through_trusted_intermediary() {
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let c_id = named_instance("C");
        let mut nodes = vec![
            SimNode::new(a_id.clone()),
            SimNode::new(b_id.clone()),
            SimNode::new(c_id.clone()),
        ];
        // A → B trusted; B → C trusted; A → C NOT trusted.
        nodes[0].serve_embargo_to(&b_id);
        nodes[1].serve_embargo_to(&c_id);

        let secret = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "x",
            embargoed: true,
        });

        // Replicate via the chain A→B→C.
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        replicate_until_quiescent(&mut nodes, &[(1, 2)], &project, 4);

        // B and C have the secret (B got it from trusted A; C got it from trusted B).
        assert!(nodes[1].has(&secret));
        assert!(nodes[2].has(&secret));

        // Now try direct A→C: A does NOT trust C, so no delivery.
        // Reset C to test the direct path in isolation.
        let mut c_only = SimNode::new(c_id.clone());
        c_only.replicate_from(&nodes[0], &project);
        assert!(
            !c_only.has(&secret),
            "direct A→C should not deliver embargoed"
        );
    }

    /// Asymmetric trust does not affect non-embargoed delivery.
    #[test]
    fn asymmetric_trust_does_not_block_public() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];
        // No embargo trust at all.
        let v = nodes[1].create(CreateArgs {
            project_id: &project,
            slug: "p",
            payload: "open",
            embargoed: false,
        });
        replicate_until_quiescent(&mut nodes, &[(1, 0)], &project, 4);
        assert!(nodes[0].has(&v));
    }

    #[test]
    fn rename_preserves_origin_id() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];

        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "old-slug",
            payload: "v1",
            embargoed: false,
        });
        let v2 = nodes[0].rename(RenameArgs {
            previous: v1.clone(),
            project_id: &project,
            new_slug: "new-slug",
            payload: "v1",
            embargoed: false,
        });

        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);

        assert!(nodes[1].has(&v1));
        assert!(nodes[1].has(&v2));
        // origin_id is the same; slug differs.
        assert_eq!(v1.origin_id, v2.origin_id);
        let e1 = nodes[1].entries.get(&entry_key(&v1)).unwrap();
        let e2 = nodes[1].entries.get(&entry_key(&v2)).unwrap();
        assert_eq!(e1.entry.meta.slug.as_str(), "old-slug");
        assert_eq!(e2.entry.meta.slug.as_str(), "new-slug");
    }

    #[test]
    fn tombstone_replicates() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];
        let v1 = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "foo",
            payload: "v1",
            embargoed: false,
        });
        let v2 = nodes[0].delete(DeleteArgs {
            previous: v1.clone(),
            project_id: &project,
            slug: "foo",
            embargoed: false,
        });
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        let stored = nodes[1].entries.get(&entry_key(&v2)).unwrap();
        assert_eq!(stored.entry.header.kind, JournalKind::Tombstone);
    }

    #[test]
    fn dedup_when_entry_arrives_via_two_paths() {
        // A→B→C and A→C; C should end up with one copy of each entry.
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
            SimNode::new(named_instance("C")),
        ];

        for i in 0..5 {
            nodes[0].create(CreateArgs {
                project_id: &project,
                slug: &format!("slug-{i}"),
                payload: "p",
                embargoed: false,
            });
        }

        // Replicate A→B, then A→C and B→C.
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        replicate_until_quiescent(&mut nodes, &[(0, 2), (1, 2)], &project, 4);

        assert_eq!(nodes[2].count(), 5);
        assert_dedup_per_node(&nodes);
    }

    // ── Replication path tests ──────────────────────────────────────────

    #[test]
    fn locally_authored_path_is_self() {
        let project = test_project();
        let a_id = named_instance("A");
        let mut a = SimNode::new(a_id.clone());
        let v = a.create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        let path = a.path_for(&v).unwrap();
        assert_eq!(path, vec![a_id]);
    }

    #[test]
    fn single_hop_path_is_origin_then_self() {
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let mut nodes = vec![SimNode::new(a_id.clone()), SimNode::new(b_id.clone())];
        let v = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        let path = nodes[1].path_for(&v).unwrap();
        assert_eq!(path, vec![a_id, b_id]);
    }

    #[test]
    fn chain_path_records_each_relay() {
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let c_id = named_instance("C");
        let mut nodes = vec![
            SimNode::new(a_id.clone()),
            SimNode::new(b_id.clone()),
            SimNode::new(c_id.clone()),
        ];
        let v = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        replicate_until_quiescent(&mut nodes, &[(1, 2)], &project, 4);
        let path = nodes[2].path_for(&v).unwrap();
        assert_eq!(path, vec![a_id, b_id, c_id]);
    }

    #[test]
    fn first_received_path_wins_under_dedup() {
        // C replicates from B (which already has it from A), then from A directly.
        // The direct path arrives second and is dedup'd — first path persists.
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let c_id = named_instance("C");
        let mut nodes = vec![
            SimNode::new(a_id.clone()),
            SimNode::new(b_id.clone()),
            SimNode::new(c_id.clone()),
        ];
        let v = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        // A→B, then B→C, then A→C.
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);
        replicate_until_quiescent(&mut nodes, &[(1, 2)], &project, 4);
        replicate_until_quiescent(&mut nodes, &[(0, 2)], &project, 4);
        let path = nodes[2].path_for(&v).unwrap();
        // C kept the first-received path: [A, B, C].
        assert_eq!(path, vec![a_id, b_id, c_id]);
    }

    /// All entries on a node must end with this node's instance_id.
    fn assert_path_ends_with_self(nodes: &[SimNode]) {
        for node in nodes {
            for se in node.entries.values() {
                assert_eq!(
                    se.path.last(),
                    Some(&node.instance_id),
                    "path on {} doesn't end with self: {:?}",
                    node.instance_id,
                    se.path,
                );
            }
        }
    }

    /// Every path's first element is the writing instance_id (origin_instance_id).
    fn assert_path_starts_with_origin(nodes: &[SimNode]) {
        for node in nodes {
            for se in node.entries.values() {
                assert_eq!(
                    se.path.first(),
                    Some(&se.entry.header.version.origin_instance_id),
                    "path on {} doesn't start with origin: {:?}",
                    node.instance_id,
                    se.path,
                );
            }
        }
    }

    #[test]
    fn mesh_paths_satisfy_invariants() {
        let project = test_project();
        let names = ["A", "B", "C", "D"];
        let ids: Vec<InstanceId> = names.iter().map(|n| named_instance(n)).collect();
        let mut nodes: Vec<SimNode> = ids.iter().cloned().map(SimNode::new).collect();
        for (i, id) in ids.iter().enumerate() {
            nodes[i].create(CreateArgs {
                project_id: &project,
                slug: &names[i].to_ascii_lowercase(),
                payload: "",
                embargoed: false,
            });
            let _ = id; // silence unused
        }
        replicate_until_quiescent(&mut nodes, &mesh_edges(4), &project, 20);
        assert_path_ends_with_self(&nodes);
        assert_path_starts_with_origin(&nodes);
    }

    // ── Concurrent replication streams ──────────────────────────────────

    /// Two pulls from the same upstream prepared concurrently — both based
    /// on the same cursor — applied sequentially. Dedup at the receiver
    /// prevents duplicate insertion, and the cursor ends at the higher
    /// new_cursor.
    #[test]
    fn two_concurrent_pulls_same_upstream() {
        let project = test_project();
        let a_id = named_instance("A");
        let mut a = SimNode::new(a_id.clone());
        let mut b = SimNode::new(named_instance("B"));

        a.create(CreateArgs {
            project_id: &project,
            slug: "s1",
            payload: "",
            embargoed: false,
        });
        a.create(CreateArgs {
            project_id: &project,
            slug: "s2",
            payload: "",
            embargoed: false,
        });

        // Two pulls prepared at the same cursor (0).
        let batch1 = b.prepare_pull_from(&a, &project);
        let batch2 = b.prepare_pull_from(&a, &project);
        assert_eq!(batch1.len(), 2);
        assert_eq!(batch2.len(), 2);

        // Apply both. The second should accept zero new entries (dedup).
        let n1 = b.apply_pull(batch1);
        let n2 = b.apply_pull(batch2);
        assert_eq!(n1, 2);
        assert_eq!(n2, 0);
        assert_eq!(b.count(), 2);
    }

    /// Two pulls from the same upstream applied in REVERSE order (newer
    /// batch first). The cursor must not regress; older batch's cursor
    /// update is dominated by max().
    #[test]
    fn out_of_order_apply_does_not_regress_cursor() {
        let project = test_project();
        let a_id = named_instance("A");
        let mut a = SimNode::new(a_id.clone());
        let mut b = SimNode::new(named_instance("B"));

        a.create(CreateArgs {
            project_id: &project,
            slug: "s1",
            payload: "",
            embargoed: false,
        });

        // Prepare batch1 at cursor=0.
        let batch1 = b.prepare_pull_from(&a, &project);
        // Apply batch1: cursor advances.
        b.apply_pull(batch1);
        let cursor_after_first = b
            .cursors
            .get(&(a_id.clone(), project.clone()))
            .copied()
            .unwrap();
        assert!(cursor_after_first.get() > 0);

        // A produces another entry.
        a.create(CreateArgs {
            project_id: &project,
            slug: "s2",
            payload: "",
            embargoed: false,
        });

        // Prepare two batches at the new cursor.
        let batch_a = b.prepare_pull_from(&a, &project);
        let batch_b = b.prepare_pull_from(&a, &project);

        // Apply in reverse order: batch_b first, then batch_a.
        let n_b = b.apply_pull(batch_b);
        let cursor_after_b = b
            .cursors
            .get(&(a_id.clone(), project.clone()))
            .copied()
            .unwrap();
        let n_a = b.apply_pull(batch_a);
        let cursor_after_a = b.cursors.get(&(a_id, project)).copied().unwrap();

        assert_eq!(n_b, 1);
        assert_eq!(n_a, 0); // dedup
        // Cursor never regresses.
        assert_eq!(cursor_after_a, cursor_after_b);
    }

    /// C replicates from both A and B concurrently, where the same entry
    /// (originated at A) reaches C via two paths. Dedup keeps it once.
    #[test]
    fn concurrent_pulls_from_two_upstreams_same_entry() {
        let project = test_project();
        let a_id = named_instance("A");
        let b_id = named_instance("B");
        let mut nodes = vec![
            SimNode::new(a_id.clone()),
            SimNode::new(b_id.clone()),
            SimNode::new(named_instance("C")),
        ];

        let v = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "s1",
            payload: "",
            embargoed: false,
        });

        // Get v onto B first.
        replicate_until_quiescent(&mut nodes, &[(0, 1)], &project, 4);

        // Concurrently prepare C's pulls from A and B.
        let batch_from_a = nodes[2].prepare_pull_from(&nodes[0], &project);
        let batch_from_b = nodes[2].prepare_pull_from(&nodes[1], &project);

        // Apply both.
        let n_a = nodes[2].apply_pull(batch_from_a);
        let n_b = nodes[2].apply_pull(batch_from_b);

        // C accepts v from one source; the other dedups.
        assert_eq!(n_a + n_b, 1);
        assert!(nodes[2].has(&v));
        assert_eq!(nodes[2].count(), 1);
    }

    /// While a pull is in-flight, the upstream commits new entries.
    /// The in-flight batch reflects the snapshot at prep time; a follow-up
    /// pull picks up the new entries.
    #[test]
    fn inflight_pull_does_not_see_late_writes() {
        let project = test_project();
        let mut a = SimNode::new(named_instance("A"));
        let mut b = SimNode::new(named_instance("B"));

        a.create(CreateArgs {
            project_id: &project,
            slug: "s1",
            payload: "",
            embargoed: false,
        });

        // B prepares pull (sees r1 only).
        let batch = b.prepare_pull_from(&a, &project);
        assert_eq!(batch.len(), 1);

        // A writes more entries while B's pull is in-flight.
        a.create(CreateArgs {
            project_id: &project,
            slug: "s2",
            payload: "",
            embargoed: false,
        });

        // B applies the in-flight batch — only r1.
        b.apply_pull(batch);
        assert_eq!(b.count(), 1);

        // Follow-up pull picks up r2.
        b.replicate_from(&a, &project);
        assert_eq!(b.count(), 2);
    }

    #[test]
    fn three_node_mesh_reaches_consistency() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
            SimNode::new(named_instance("C")),
        ];
        nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        nodes[1].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        nodes[2].create(CreateArgs {
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });

        replicate_until_quiescent(&mut nodes, &mesh_edges(3), &project, 20);

        for i in 0..3 {
            assert_eq!(nodes[i].count(), 3, "node {} has wrong count", i);
        }
        assert_all_non_embargoed_everywhere_in_mesh(&nodes, &project);
    }

    #[test]
    fn four_node_mesh_with_mixed_embargo() {
        let project = test_project();
        let names = ["A", "B", "C", "D"];
        let ids: Vec<InstanceId> = names.iter().map(|n| named_instance(n)).collect();
        let mut nodes: Vec<SimNode> = ids.iter().cloned().map(SimNode::new).collect();

        // Trust: A↔B trust each other for embargo. C, D do not.
        nodes[0].serve_embargo_to(&ids[1]);
        nodes[1].serve_embargo_to(&ids[0]);

        // A creates an embargoed entry and a public entry.
        let secret = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "secret",
            payload: "",
            embargoed: true,
        });
        let public = nodes[0].create(CreateArgs {
            project_id: &project,
            slug: "public",
            payload: "",
            embargoed: false,
        });

        replicate_until_quiescent(&mut nodes, &mesh_edges(4), &project, 20);

        // Every node has the public entry.
        for (i, n) in nodes.iter().enumerate() {
            assert!(n.has(&public), "{} missing public", names[i]);
        }
        // Only A and B have the secret.
        assert!(nodes[0].has(&secret));
        assert!(nodes[1].has(&secret));
        assert!(!nodes[2].has(&secret));
        assert!(!nodes[3].has(&secret));

        assert_dedup_per_node(&nodes);
    }

    // ── Property-ish randomised tests ────────────────────────────────────

    /// Apply a sequence of random creates/edits/replicates on `n` nodes.
    /// After quiescence, invariants must hold.
    fn random_workload(seed: u64, n_nodes: usize, n_ops: usize) {
        let mut rng = StdRng::seed_from_u64(seed);
        let project = test_project();

        let ids: Vec<InstanceId> = (0..n_nodes)
            .map(|i| named_instance(&format!("N{i}")))
            .collect();
        let mut nodes: Vec<SimNode> = ids.iter().cloned().map(SimNode::new).collect();

        // Random trust config.
        for i in 0..n_nodes {
            for j in 0..n_nodes {
                if i == j {
                    continue;
                }
                if rng.gen_bool(0.5) {
                    let peer = ids[j].clone();
                    nodes[i].serve_embargo_to(&peer);
                }
            }
        }

        // Track every federated version created so we can pick previous_versions
        // for edits.
        let mut all_versions: Vec<FederatedVersion> = Vec::new();

        let edges = mesh_edges(n_nodes);

        for op_idx in 0..n_ops {
            let op = rng.gen_range(0..4);
            match op {
                0 => {
                    // Create
                    let node_idx = rng.gen_range(0..n_nodes);
                    let embargoed = rng.gen_bool(0.3);
                    let v = nodes[node_idx].create(CreateArgs {
                        project_id: &project,
                        slug: &format!("slug-{op_idx}"),
                        payload: "",
                        embargoed,
                    });
                    all_versions.push(v);
                }
                1 => {
                    // Edit a known version — pick one some node has.
                    if let Some(prev) = all_versions.choose(&mut rng).cloned() {
                        // Find any node that has this entry.
                        let holders: Vec<usize> =
                            (0..n_nodes).filter(|&i| nodes[i].has(&prev)).collect();
                        if let Some(&editor) = holders.choose(&mut rng) {
                            let embargoed = rng.gen_bool(0.2);
                            let v = nodes[editor].edit(EditArgs {
                                previous: prev,
                                project_id: &project,
                                slug: &format!("slug-e{op_idx}"),
                                payload: "",
                                embargoed,
                            });
                            all_versions.push(v);
                        }
                    }
                }
                2 => {
                    // One random replication hop.
                    let from = rng.gen_range(0..n_nodes);
                    let to = loop {
                        let t = rng.gen_range(0..n_nodes);
                        if t != from {
                            break t;
                        }
                    };
                    let (from_n, to_n) = get_two_mut(&mut nodes, from, to);
                    to_n.replicate_from(from_n, &project);
                }
                _ => {
                    // Rename.
                    if let Some(prev) = all_versions.choose(&mut rng).cloned() {
                        let holders: Vec<usize> =
                            (0..n_nodes).filter(|&i| nodes[i].has(&prev)).collect();
                        if let Some(&editor) = holders.choose(&mut rng) {
                            let v = nodes[editor].rename(RenameArgs {
                                previous: prev,
                                project_id: &project,
                                new_slug: &format!("renamed-{op_idx}"),
                                payload: "",
                                embargoed: false,
                            });
                            all_versions.push(v);
                        }
                    }
                }
            }
        }

        // Final quiescence with full mesh.
        replicate_until_quiescent(&mut nodes, &edges, &project, 50);

        // Invariants.
        assert_dedup_per_node(&nodes);
        let embargo_map = global_entry_embargo_map(&nodes);
        assert_previous_version_or_embargo_gap(&nodes, &embargo_map);
    }

    #[test]
    fn random_3_nodes_50_ops() {
        for seed in 0..20 {
            random_workload(seed, 3, 50);
        }
    }

    #[test]
    fn random_4_nodes_100_ops() {
        for seed in 0..10 {
            random_workload(seed, 4, 100);
        }
    }
}
