# TLA+ specification of the replication protocol

Formal model of the protocol described in [../designdocs/Replication.md](../designdocs/Replication.md), checked with [TLC](https://lamport.azurewebsites.net/tla/tla.html).

Each entry carries two version fields: `version` (paired with `origin_id` to form the federated identity, unique per resource at its origin) and `localVersion` (the commit-order position on whichever node currently holds the entry — for locally-authored entries it equals `version`; for replicated entries it is the receiver's own txid).
Pull filters and the cursor are defined over `localVersion`, not `version`, because a node that forwards replicated content needs a single ordering that interleaves both locally-authored and replicated entries.
Filtering on `version` would skip entries the node replicated from a third party.

## Running

The model runs under Bazel with a hermetic JRE + `tla2tools.jar` fetched on first build:

```sh
bazel test //tla:replication_tlc_test           # ~6s
bazel test //tla:replication_deep_tlc_test      # ~30s, larger state space
```

## What the spec covers

Actions:

- `BeginNewPlan` — open a transaction, write a brand-new resource entry with no parent.
- `BeginNewComment` — open a transaction, write a cross-resource entry whose `parentRef` points at a committed parent on this node.
- `WriteCommentInOpenPlanTxn` — write a cross-resource entry into an already-open transaction whose first entry was a plan; the comment shares the parent's `ver`/`localVersion` (models Postgres read-your-writes within a txn).
- `BeginEdit` — open a transaction, edit an existing committed entry (same-resource chain).
- `Commit` — commit an in-flight transaction; all entries with that txid become visible.
- `BatchPull` — receiver pulls a batch from upstream; `ValidBatch` constrains the batch to a prefix in `localVersion` order, ancestor-closed over both `prev` and `parentRef`.

Two ResourceIds (`plan`, `comment`).
`BeginNewComment` / `WriteCommentInOpenPlanTxn` together capture "writer must have read the parent" — committed locally, or visible in the same open transaction.
Without this precondition, parent-before-child would not hold under arbitrary relay topologies.

Invariants:

- **`NoDuplicates`** — federated identity `(origin, rid, ver)` is unique per node.
- **`PathStartsWithOrigin`** / **`PathEndsWithSelf`** — path tracking invariants.
- **`EmbargoFilter`** — embargoed entries traverse only trusted hops.
- **`ChainIntactOrEmbargoGap`** — a committed entry's `prev` resolves locally or points at an embargoed entry.
- **`ParentRefIntactOrEmbargoGap`** — cross-resource analogue for `parentRef`.
- **`NoCommittedEntryLost`** — no committed entry is skipped by cursor advance; the property the watermark wind-back exists to preserve.
- **`ProjectIsolation`** / **`TypeOK`** — structural sanity.

## Observed state-space sizes

On a devcontainer with `-workers auto` and `SYMMETRY Permutations({NodeA, NodeB})`
(NodeC is asymmetric in the trust matrix so it's not interchangeable):

| Run    | MaxOps | BatchSize | Distinct states | Wall time |
|--------|--------|-----------|-----------------|-----------|
| Fast   | 7      | 2         | 807 367         | ~6 s      |
| Deep   | 8      | 1         | 4 626 498       | ~30 s     |

## Limitations

- Single project — the MC configs bind `MCProjects == {"P1"}`; cross-project behaviour is not modelled here
- Tombstones (`kind = TOMBSTONE`) not modelled — the journal has only creates and edits
- No liveness — only safety invariants are checked; eventual-consistency claims are not verified
- Trust matrix is static
- One depth level of cross-resource references (comments reference plans, not other comments)
