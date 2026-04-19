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
//! - Embargo trust is symmetric and binary per pair.

#![allow(dead_code)] // scaffolding for tests

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;

use sqlx::types::chrono;
use uuid::Uuid;

use crate::journal::{
    FederatedIdentity, FederatedVersion, InstanceId, JournalEntryHeader, JournalKind,
    ResourceEntryMeta,
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
    local_version: u64,
    /// Safe resume point for replication. Simplified: equal to local_version.
    watermark: u64,
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
    /// Next local_version to assign.
    next_local_version: u64,
    /// Replication cursor per (upstream, project).
    cursors: HashMap<(InstanceId, ProjectId), u64>,
    /// Peers this node will serve embargoed entries to (outbound trust).
    serves_embargo_to: HashSet<InstanceId>,
}

type EntryKey = (InstanceId, String, u64);

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
            next_local_version: 1,
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
            resource_id: args.resource_id,
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
        let resource_id = args.previous.origin_id.clone();
        self.write(WriteArgs {
            resource_id: &resource_id,
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
        let resource_id = args.previous.origin_id.clone();
        self.write(WriteArgs {
            resource_id: &resource_id,
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
        let resource_id = args.previous.origin_id.clone();
        self.write(WriteArgs {
            resource_id: &resource_id,
            project_id: args.project_id,
            slug: args.slug,
            payload: "",
            embargoed: args.embargoed,
            kind: JournalKind::Tombstone,
            previous_version: Some(args.previous),
        })
    }

    fn write(&mut self, args: WriteArgs) -> FederatedVersion {
        let local_version = self.next_local_version;
        self.next_local_version += 1;

        let version = FederatedVersion {
            origin_instance_id: self.instance_id.clone(),
            origin_id: args.resource_id.to_string(),
            version: NonZeroU64::new(local_version).unwrap(),
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
        let cursor_key = (other.instance_id.clone(), project_id.clone());
        let cursor = self.cursors.get(&cursor_key).copied().unwrap_or(0);

        let serve_embargoed = other.serves_embargo_to.contains(&self.instance_id);

        // Select entries in `other` for this project with local_version > cursor,
        // ordered by local_version (topological by construction — local_version
        // is monotonic in commit order).
        let mut to_send: Vec<&SimStoredEntry> = other
            .entries
            .values()
            .filter(|se| &se.entry.meta.project_id == project_id)
            .filter(|se| se.local_version > cursor)
            .collect();
        to_send.sort_by_key(|se| se.local_version);

        let mut accepted = 0;
        let mut max_watermark = cursor;

        for se in to_send {
            // Always advance max_watermark — filtered entries still
            // contribute to the cursor.
            if se.watermark > max_watermark {
                max_watermark = se.watermark;
            }

            // Filter: requester originated this entry.
            if se.entry.header.version.origin_instance_id == self.instance_id {
                continue;
            }
            // Filter: embargoed entries to untrusted peers.
            if se.entry.header.embargoed && !serve_embargoed {
                continue;
            }
            // Dedup: we already have this entry.
            let key = entry_key(&se.entry.header.version);
            if self.entries.contains_key(&key) {
                continue;
            }

            let our_local_version = self.next_local_version;
            self.next_local_version += 1;
            // Build new path: source's path + this instance.
            let mut new_path = se.path.clone();
            new_path.push(self.instance_id.clone());
            self.entries.insert(
                key,
                SimStoredEntry {
                    entry: se.entry.clone(),
                    local_version: our_local_version,
                    watermark: our_local_version,
                    path: new_path,
                },
            );
            accepted += 1;
        }

        self.cursors.insert(cursor_key, max_watermark);
        accepted
    }
}

// ── Write operation argument structs ─────────────────────────────────────

pub struct CreateArgs<'a> {
    pub resource_id: &'a str,
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
    resource_id: &'a str,
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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
            resource_id: "res-2",
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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

    #[test]
    fn rename_preserves_origin_id() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];

        let v1 = nodes[0].create(CreateArgs {
            resource_id: "res-1",
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
        assert_eq!(e1.entry.meta.slug, "old-slug");
        assert_eq!(e2.entry.meta.slug, "new-slug");
    }

    #[test]
    fn tombstone_replicates() {
        let project = test_project();
        let mut nodes = vec![
            SimNode::new(named_instance("A")),
            SimNode::new(named_instance("B")),
        ];
        let v1 = nodes[0].create(CreateArgs {
            resource_id: "res-1",
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
                resource_id: &format!("res-{i}"),
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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
            resource_id: "res-1",
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
                resource_id: &format!("res-{}", names[i]),
                project_id: &project,
                slug: names[i],
                payload: "",
                embargoed: false,
            });
            let _ = id; // silence unused
        }
        replicate_until_quiescent(&mut nodes, &mesh_edges(4), &project, 20);
        assert_path_ends_with_self(&nodes);
        assert_path_starts_with_origin(&nodes);
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
            resource_id: "a1",
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        nodes[1].create(CreateArgs {
            resource_id: "b1",
            project_id: &project,
            slug: "s",
            payload: "",
            embargoed: false,
        });
        nodes[2].create(CreateArgs {
            resource_id: "c1",
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
            resource_id: "secret-1",
            project_id: &project,
            slug: "secret",
            payload: "",
            embargoed: true,
        });
        let public = nodes[0].create(CreateArgs {
            resource_id: "public-1",
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
                    let rid = format!("res-{op_idx}");
                    let embargoed = rng.gen_bool(0.3);
                    let v = nodes[node_idx].create(CreateArgs {
                        resource_id: &rid,
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
