# TLA+ specification of the replication protocol

`Replication.tla` is a formal model of the protocol described in [../Replication.md](../Replication.md), small enough to fit in a finite state space yet rich enough to exercise the interesting interactions: dedup under multi-path delivery, embargo filtering with asymmetric trust, the previous-version chain across instances, and replication path tracking.

The model intentionally does **not** include out-of-order commits / watermark mechanics.
Versions are atomic and monotonic per node.
The watermark behaviour is exercised in the Rust `txn_sim` module instead, which can model concurrent transactions cheaply.

## Running the model checker

Prerequisite: a JRE 11 or later.
On Debian/Ubuntu:

```sh
sudo apt install openjdk-21-jre-headless
```

Then:

```sh
designdocs/tla/run_tlc.sh
```

The script downloads `tla2tools.jar` to `~/.cache/tla/` on first run and invokes TLC on `Replication.tla` with `Replication.cfg`.

To pass extra TLC flags:

```sh
designdocs/tla/run_tlc.sh -workers auto
```

## What the spec covers

Constants (configured in `Replication.cfg`):

- `Nodes = {"A", "B", "C"}`
- `Projects = {"P1"}`
- `ResourceIds = {"r1"}`
- `Trust`: `A` and `B` serve embargoed content to each other; `C` is excluded
- `MaxOps = 4`: cap on operations to keep the state space finite

Actions:

- `Create(node, rid, proj, emb)`: write a new resource on `node`
- `Edit(node, prev)`: write a follow-up referencing an existing entry on this node (cross-instance edits create a new origin entry on the editor)
- `Pull(receiver, upstream, proj)`: receiver pulls eligible entries from upstream — applies embargo trust, dedup, and origin filter

Invariants checked at every reachable state:

- `NoDuplicates`: no two entries on a node share `(origin, rid, ver)`
- `PathStartsWithOrigin`: every stored entry's path begins with its origin
- `PathEndsWithSelf`: every stored entry's path ends with the storing node
- `EmbargoFilter`: embargoed entries traverse only trusted hops
- `ProjectIsolation`: every entry has a valid project
- `ChainIntactOrEmbargoGap`: previous-version references resolve locally OR the referenced entry is embargoed (acceptable gap)
- `TypeOK`: type-correctness of all variables

## State space size

Observed at time of writing on a devcontainer with 32 cores:

| MaxOps | Distinct states | Wall time    |
|--------|-----------------|--------------|
| 4      | 1 147           | < 1 s        |
| 5      | 6 035           | < 1 s        |
| 6      | 34 504          | ~2 s         |

The default is 6.
Higher values scale roughly 5-6× per added op; beyond 8 or 9 ops, TLC starts needing more time and memory.

Increasing `Nodes` grows the space faster still.
For exhaustive runs over a bigger space, pass `-workers auto` to parallelise.

## Limitations vs. the protocol spec

- No watermark / out-of-order commits (use `txn_sim` for that)
- No tombstone op (single `kind = entry` only)
- Trust matrix is configuration-time; runtime trust changes not modelled
- No liveness assertions (model checker can be extended with `[]<>` for eventual consistency under fairness; deferred)

These limitations let the spec stay focused on safety properties of the routing/dedup/embargo logic.
