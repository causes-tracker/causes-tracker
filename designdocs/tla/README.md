# TLA+ specification of the replication protocol

Formal models of the protocol described in [../Replication.md](../Replication.md), checked with [TLC](https://lamport.azurewebsites.net/tla/tla.html).

Two models with increasing fidelity:

- [`Replication.tla`](Replication.tla) — routing-only.
  Versions are a simple per-node counter with atomic assignment.
  Fast; checks dedup, path, embargo, chain.

- [`ReplicationTxn.tla`](ReplicationTxn.tla) — protocol-faithful.
  Models transaction lifecycle (begin/insert/commit), allowing out-of-order commits.
  Adds `NoCommittedEntryLost` to verify the watermark cursor doesn't permanently skip late-committing entries.
  Admits the Postgres reference implementation.

Each entry carries two version fields: `version` (paired with `origin_id` to form the federated identity, unique per resource at its origin) and `localVersion` (the receiver's commit-order position).
Pull filters and the cursor are defined over `localVersion`, not `version`, because a receiver that forwards replicated content needs a single ordering that interleaves both locally-authored and replicated entries.
Filtering on `version` would skip entries the receiver replicated from a third party.

## Running

Both models run under Bazel with a hermetic JRE + tla2tools.jar fetched on first build:

```sh
# Fast — always in CI.
bazel test //designdocs/tla:replication_tlc_test
bazel test //designdocs/tla:replication_txn_tlc_test

# Deeper — manual.
bazel test //designdocs/tla:replication_txn_deep_tlc_test
```

For ad-hoc runs outside Bazel (e.g. when iterating on the spec), use the shell wrapper, which requires Java locally and caches `tla2tools.jar` in `~/.cache/tla`:

```sh
designdocs/tla/run_tlc.sh                 # defaults to Replication model
```

## What the specs cover

### Routing-only model (`Replication.tla`)

Actions: `Create`, `Edit`, `Pull`.
Invariants: `NoDuplicates`, `PathStartsWithOrigin`, `PathEndsWithSelf`, `EmbargoFilter`, `ProjectIsolation`, `ChainIntactOrEmbargoGap`, `TypeOK`.

### Txn model (`ReplicationTxn.tla`)

Adds: each insert allocates a txid and is uncommitted until `Commit(n, v)` runs.
Multiple transactions can be in-flight concurrently; commits can interleave with other commits and pulls.
Each row carries its own `localVersion` (per-instance commit-order position) and `watermark` (lowest in-flight version at insert time).
Pull filters on `localVersion` and advances the cursor to `max(watermark)` of the visible set.

Extra invariant:

- **`NoCommittedEntryLost`** — for any committed entry on an upstream with `localVersion < cursor[receiver][upstream][project]`, either the receiver has it, or the receiver is the origin, or the entry is embargoed to an untrusted peer.
  This is precisely the property the watermark mechanism exists to preserve: cursor advance must never skip a late-committing entry.

## Observed state-space sizes

On a devcontainer with `-workers auto` and `SYMMETRY Permutations({NodeA, NodeB})`
(NodeC is asymmetric in the trust matrix so it's not interchangeable):

| Model                         | MaxOps | Distinct states | Wall time |
|-------------------------------|--------|-----------------|-----------|
| `Replication`                 | 6      | 17 596          | ~1 s      |
| `ReplicationTxn` (CI)         | 9      | 272 559         | ~2 s      |
| `ReplicationTxnDeep` (manual) | 10     | 1 450 051       | ~8 s      |

## Limitations

- Single project — multi-project behaviour is modelled by the Rust `replication_sim` instead, which is faster to iterate
- No tombstone op (entries are either present or not)
- No liveness (TLC `[]<>` supported but not used — eventual-consistency assertions live in the Rust sim's quiescence checks)
- Trust matrix is static
