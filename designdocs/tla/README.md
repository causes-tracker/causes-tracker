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

The routing-only model existed first.
Moving to the txn model caught a real modelling error: my first attempt filtered replication on origin-version rather than local-version, which would have lost entries when receivers forwarded replicated content.
The counterexample trace pointed straight at the confused identifier; the fix matched the reference implementation's separation of `origin_id+version` (federated identity) from `local_version` (per-node commit order).

## Running

Both models run under Bazel with a hermetic JRE + tla2tools.jar fetched on first build:

```sh
# Fast — always in CI.
bazel test //designdocs/tla:replication_tlc_test
bazel test //designdocs/tla:replication_txn_tlc_test

# Deeper — manual (~17s).
bazel test //designdocs/tla:replication_txn_deep_tlc_test
```

For ad-hoc runs outside Bazel (e.g. when iterating on the spec), use the shell wrapper, which requires Java locally and caches `tla2tools.jar` in `~/.cache/tla`:

```sh
designdocs/tla/run_tlc.sh                 # defaults to Replication model
```

Or run `bazel test` as above for the Bazel-hermetic path.

## What the specs cover

### Routing-only model (`Replication.tla`)

Actions: `Create`, `Edit`, `Pull`.
Invariants: `NoDuplicates`, `PathStartsWithOrigin`, `PathEndsWithSelf`, `EmbargoFilter`, `ProjectIsolation`, `ChainIntactOrEmbargoGap`, `TypeOK`.

### Txn model (`ReplicationTxn.tla`)

Adds: each insert allocates a txid and is uncommitted until `Commit(n, v)` runs.
Multiple transactions can be in-flight concurrently; commits can interleave with other commits and pulls.
Each row carries its own `localVersion` (per-instance commit-order position) and `watermark` (lowest in-flight version at insert time).
Pull filters on `localVersion` and advances the cursor to `max(watermark)` of the visible set.

Extra invariant: **`NoCommittedEntryLost`** — for any committed entry on an upstream with `localVersion < cursor[receiver][upstream][project]`, either the receiver has it, or the receiver is the origin, or the entry is embargoed to an untrusted peer.
This is precisely the property the watermark mechanism exists to preserve: cursor advance must never skip a late-committing entry.

## Observed state-space sizes

On a devcontainer with `-workers auto`:

| Model                          | MaxOps | Distinct states | Wall time |
|--------------------------------|--------|-----------------|-----------|
| `Replication`                  | 4      | 1 147           | < 1 s     |
| `Replication`                  | 6      | 34 504          | ~2 s      |
| `ReplicationTxn`               | 7      | 15 011          | ~3 s      |
| `ReplicationTxn`               | 8      | 109 184         | ~3 s      |
| `ReplicationTxn`               | 9 (CI) | 540 853         | ~5 s      |
| `ReplicationTxnDeep`           | 10     | 2 883 330       | ~17 s     |

## Limitations

- Single project, single resource id — combinations of multiple projects/resources are modelled by the Rust `replication_sim` instead, which is faster to iterate
- No tombstone op (entries are either present or not)
- No liveness (TLC `[]<>` supported but not used — eventual-consistency assertions live in the Rust sim's quiescence checks)
- Trust matrix is static
