# TLA+ specification of the replication protocol

Formal models of the protocol described in [../Replication.md](../Replication.md), checked with [TLC](https://lamport.azurewebsites.net/tla/tla.html).

## Routing-only model (`Replication.tla`)

Versions are a simple per-node counter with atomic assignment.
Fast; checks dedup, path, embargo, chain.

Actions: `Create`, `Edit`, `Pull`.
Invariants: `NoDuplicates`, `PathStartsWithOrigin`, `PathEndsWithSelf`, `EmbargoFilter`, `ProjectIsolation`, `ChainIntactOrEmbargoGap`, `TypeOK`.

## Running

```sh
bazel test //designdocs/tla:replication_tlc_test
```

For ad-hoc runs outside Bazel (requires Java locally; caches `tla2tools.jar` in `~/.cache/tla`):

```sh
designdocs/tla/run_tlc.sh                 # defaults to Replication model
```

## Observed state-space sizes

On a devcontainer with `-workers auto` and `SYMMETRY Permutations({NodeA, NodeB})`
(NodeC is asymmetric in the trust matrix so it's not interchangeable):

| Model         | MaxOps | Distinct states | Wall time |
|---------------|--------|-----------------|-----------|
| `Replication` | 6      | 17 596          | ~1 s      |

## Limitations

- Single project — multi-project behaviour is modelled by the Rust `replication_sim` instead, which is faster to iterate
- No tombstone op (entries are either present or not)
- No liveness (TLC `[]<>` supported but not used — eventual-consistency assertions live in the Rust sim's quiescence checks)
- Trust matrix is static
