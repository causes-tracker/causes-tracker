# Replication Protocol

This document specifies the Causes replication protocol.
It is the specification for implementors building a Causes-compatible instance or alternative storage backend.

For the design rationale behind these decisions, see [ADR-013](Decisions.md#adr-013-replication-protocol) in Decisions.md.
For the broader federation strategy, see [ADR-006](Decisions.md#adr-006-federation-strategy--distribute-dont-federate-by-default).
For a machine-checked TLA+ model of the protocol's safety properties, see [tla/Replication.tla](tla/Replication.tla).

## Overview

Causes uses pull-based replication to distribute data between instances.
Any instance can replicate from any other by initiating an outbound connection and pulling journal entries.
No instance needs a public URL to replicate from an upstream — all connections are client-initiated.

Replication operates at the project level: a downstream instance subscribes to specific projects on an upstream and receives all journal entries for those projects.

## Data model

### Layered structure

Journal entries have a layered structure separating immutable, per-entry, and instance-local data.
The layers, from innermost to outermost:

**JournalEntryHeader** — immutable distributed identity and metadata for a single entry:

- `kind`: one of `ENTRY` or `TOMBSTONE` (see [Kind](#kind))
- `at`: timestamp of the entry
- `author`: `FederatedIdentity` — `(instance_id, local_id)` of the author
- `version`: 64-bit unsigned integer, assigned at commit time on the origin instance
- `previous_version`: `FederatedVersionRef` pointing to the prior entry for this resource (version = 0 for the first entry)
- `embargoed`: whether this entry is under embargo
- `origin_instance_id`: the instance that wrote this entry
- `origin_id`: UUID identifying the resource this entry is about

**ResourceEntryMeta** — resource-level metadata, carried in every entry:

- `slug`: human-readable identifier for the resource within its project (immutable per entry, may differ across entries for the same resource when renamed)
- `project_id`: the project this resource belongs to on the origin instance
- `created_at`: timestamp of the resource's creation, copied from the first entry into all subsequent entries

**Resource entry** (e.g. PlanEntry, SignEntry) — the complete immutable journal entry:

- `header`: JournalEntryHeader
- `meta`: ResourceEntryMeta
- Resource-specific fields (title, description, status, etc.)

**Local entry wrapper** — per-instance data, transmitted in the replication stream but replaced by the receiver:

- The resource entry itself
- `project_id`: local project filing (may differ from origin's project_id per ADR-012)
- `replication_path`: the sender transmits its replication path; the receiver appends the sender to it, building up the full delivery chain
- `local_version`: position in this instance's commit order (implementation-specific)
- `watermark`: safe resume point for replication serving (implementation-specific)

### Federated version reference

A `FederatedVersionRef` is a triple `(origin_instance_id, origin_id, version)` that uniquely identifies a single journal entry across the entire federation.
It is used for cross-resource references (e.g. "this comment is on plan X at version N").

### Kind

Journal entries have a `kind` field:

- `ENTRY`: the resource exists at this version
- `TOMBSTONE`: the resource is deleted at this version

Whether an entry represents a creation, update, or rename is derivable from context:

- Creation: `previous_version == 0`
- Update: `previous_version != 0` and slug is unchanged
- Rename: `previous_version != 0` and slug differs from the previous entry's slug
- Undelete: kind is `ENTRY` and the previous entry's kind is `TOMBSTONE`

### Tombstone retention

Tombstones are journal entries like any other — they are replicated and stored.
Discarding a tombstone requires that:

- Peers that have already received entries for the resource must also have received the tombstone.
- Peers that have never received any entry for the resource will not receive any in the future.
- The journal entry graph remains intact — no remaining entries reference the discarded entries.

### Resource identity

`origin_instance_id` is the `InstanceId` (UUID v4) of the instance that wrote the journal entry.
Different entries for the same resource may have different `origin_instance_id` values — any instance can write entries about any resource it has replicated.

`origin_id` is a UUID that identifies the resource.
It is assigned once when the resource is first created (i.e. when `previous_version` has version = 0) and is reused by all subsequent entries about that resource, regardless of which instance writes the entry.
A new `origin_id` is only allocated for genuinely new resources.
It is not human-readable; the `slug` field in ResourceEntryMeta provides the human-readable name.

The primary key for journal entries is `(origin_instance_id, origin_id, version)`.

### Slug

The `slug` in ResourceEntryMeta is the human-readable identifier for a resource within its project.
It is immutable within a single journal entry but may differ across entries for the same resource (i.e. a rename changes the slug).

### Version

Each journal entry has a `version` — a 64-bit unsigned integer assigned at commit time on the writing instance.
Versions may have gaps (not every integer is used).
Versions from different instances are independent and not comparable.
The globally unique identity of a journal entry is the `FederatedVersionRef`: `(origin_instance_id, origin_id, version)`.

See [Implementor requirements](#implementor-requirements) for the causal ordering properties that the version source must satisfy.

### Version 0

Version 0 is reserved and never assigned.
It serves as the root of every `previous_version` chain: the first entry for a resource has `previous_version = 0`.
A replication watermark of 0 means "give me everything from the beginning."

### Previous version chain

Every journal entry carries a `previous_version` field — a `FederatedVersionRef` pointing to the immediately prior entry.

- First entry: `previous_version` has version = 0 (no prior entry)
- Subsequent entries: `previous_version` points to the prior entry for this resource (same origin_id, possibly different origin_instance_id)

When instance B edits a resource that was created on instance A, B writes a new entry with `origin_instance_id = B`, the same `origin_id`, and `previous_version` pointing to the last entry (which has `origin_instance_id = A`).
The `previous_version` chain captures the full history across instances.

The chain serves three purposes:

1. **Gap detection**: if an entry's `previous_version` refers to an entry the receiver does not have, the chain has a gap.
   Gaps are normal — they arise from embargo filtering, thin clones, and partial replication.
2. **Rename tracking**: a rename is an entry where `slug` differs from the previous entry's slug.
   The chain links old and new names together without a special operation.
3. **Audit trail**: the full history of a resource is recoverable by following the chain, potentially across instances.

### Causal dependencies

When a resource references another (e.g. a comment on a plan), the reference carries `(origin_instance_id, origin_id, version)` — a pointer to a specific journal entry of the referenced resource.

The replication stream's topological ordering guarantees that referenced entries precede referencing entries.
A downstream processing a replication stream in order will always have the referenced entry before the referencing entry.

This guarantee follows from a precondition on writes: an instance can only write an entry that references another entry if it already has the referenced entry locally and committed.
Combined with monotonic local commit order, this ensures that the parent's `local_version` on any node is strictly less than the child's, so any pull serving entries in `local_version` order naturally delivers parent before child.

See [Ordering guarantees](#ordering-guarantees) for the proof.
The TLA+ model in [tla/ReplicationTxn.tla](tla/ReplicationTxn.tla) verifies this property as `ParentRefIntactOrEmbargoGap`.

## Replication protocol

### Pull RPC

Replication is driven by the downstream instance, which calls the upstream's Pull RPC:

```text
Pull(project_id, after_watermark) -> stream PullResponse
```

The request includes:

- `project_id`: which project to replicate
- `after_watermark`: resume point from a previous session (0 for initial replication)

The upstream returns a stream of journal entries for the requested project, across all resource types, ordered by the upstream's local commit order.

The upstream identifies the requesting instance from the authenticated service account (see ADR-010).
Federation service accounts carry a `remote_instance_id`; the server uses this to filter out entries where `origin_instance_id` matches the requester.
The requester originated those entries and already has them by definition.
This filtering is safe regardless of relay topology — an instance always has its own entries.

Normal user tokens do not carry a `remote_instance_id`, so no origin filtering is applied.
This allows users to query the replication stream for debugging or export without origin-based filtering.

Each response in the stream contains:

- `watermark`: the safe resume point for this entry (see [Watermark](#watermark))
- `entry`: the journal entry (JournalEntryHeader + ResourceEntryMeta + resource-specific fields)

Receivers that encounter a resource type they do not understand should abort the replication stream.
Skipping unknown entries risks dangling references in future entries that link to the skipped content.

### Watermark

The watermark is a per-(upstream, project) value that the downstream stores between replication sessions.
It represents a position in the upstream's local commit order below which all entries are guaranteed delivered.

- Initial value: 0 (start from the beginning)
- After a replication session: the downstream stores the highest `watermark` value received

On the next Pull call, the downstream passes its stored watermark to resume from where it left off.

### At-least-once delivery

The replication stream provides at-least-once delivery: the same entry may be delivered more than once across successive Pull calls.
Receivers must deduplicate on `(origin_instance_id, origin_id, version)`.

The watermark in each `PullResponse` is a safe resume point.
The downstream stores the highest watermark received and uses it as `after_watermark` for the next Pull.
The serving instance is responsible for ensuring that no entries are permanently skipped.
See [Postgres implementation notes](#postgres-implementation-notes) for how the reference implementation achieves this.

## Ordering guarantees

The replication stream is topologically ordered: if entry B depends on entry A (via `previous_version` or a cross-resource reference), then A appears before B in the stream.

This guarantee allows a downstream processing a stream in order to never encounter a dangling reference (except for gaps due to embargo filtering, thin clones, or partial replication).

The implementation must ensure this property.
See [Postgres implementation notes](#postgres-implementation-notes) for how the reference implementation achieves topological ordering and at-least-once delivery.

## Embargo

### Embargo as a journal property

The `embargoed` field is on each journal entry, not on the resource as a whole.
When a resource is embargoed, all its journal entries are marked `embargoed = true`.

Embargoed entries are filtered during replication by default.
The serving instance decides per-peer whether to include embargoed entries, based on its federation trust configuration (see ADR-010).

### Replication filtering

The serving side applies the embargo filter based on the peer's federation trust configuration.
By default, embargoed entries are excluded from the replication stream.
The mechanism for granting a peer access to embargoed content is part of the federation trust model (ADR-010), not the replication protocol.

### Un-embargo

Un-embargoing a resource creates a new journal entry with `embargoed = false`.
This entry carries the full resource content (a complete snapshot) and has `previous_version` pointing to the last embargoed entry.

The un-embargo entry is the first entry for this resource that filtered peers receive.
The `previous_version` gap is indistinguishable from other gaps (thin clones, partial replication) and is handled the same way — the receiver tolerates it.

### Transitive embargo

Embargo propagates transitively to references.
If a plan is embargoed, any comment on that plan must also be embargoed.
The server enforces this invariant at write time.
Link objects (PlanSignLink, PlanSymptomLink, PlanDependency) are embargoed whenever either linked resource is embargoed.

## Renames

A rename is a journal entry where `slug` differs from the previous entry's slug.
The resource's stable identity `(origin_instance_id, origin_id)` does not change.
The primary key `(origin_instance_id, origin_id, version)` is unaffected by renames.

The `previous_version` chain links entries across the rename.
A receiver can detect a rename by comparing the slug of an entry to the slug of the entry at `previous_version`.

## Implementor requirements

An implementation of the Causes replication protocol must satisfy the following properties.

### Topological ordering

The replication stream must be topologically ordered: if entry B depends on entry A, then A must appear before B in the stream.
Dependencies are `previous_version` links and cross-resource `FederatedVersionRef` references.

### Version source

The version source must never return 0 (reserved as the root sentinel).
Versions must be 64-bit unsigned integers.
Gaps in the version sequence are permitted.
Versions from different instances are independent and not comparable.

### Delivery guarantee

The serving instance must ensure that every entry is eventually delivered to a downstream that requests it.
The same entry may be delivered more than once.

### Deduplication

The receiver must deduplicate incoming entries on `(origin_instance_id, origin_id, version)`.
Receiving a duplicate must not corrupt state or halt replication.

## Postgres implementation notes

These details are specific to the Postgres-based implementation.
They are not protocol requirements — other implementations may use different mechanisms that satisfy the same properties.

The minimum supported PostgreSQL version is 13, which introduced `pg_current_xact_id()` (64-bit transaction IDs).
The 64-bit `xid8` type is monotonically increasing and does not wrap in practice.
The project only tests against the single PostgreSQL version pinned in the repository (currently 17).

The use of transaction IDs and snapshot visibility for change tracking in Postgres is not novel.
See [cognitedata/txid-syncing](https://github.com/cognitedata/txid-syncing) for prior work on the same technique in a different application context.

### Local version and watermark columns

Each journal table includes two Postgres-specific columns:

```sql
local_version  BIGINT NOT NULL DEFAULT pg_current_xact_id()::text::bigint
watermark      BIGINT NOT NULL DEFAULT pg_snapshot_xmin(pg_current_snapshot())::text::bigint
```

`local_version` is the transaction ID at commit time on this instance.
For locally-created entries, `local_version` equals `version` (both are the same txid).
For replicated entries, `version` is the origin's value (set explicitly) while `local_version` is the receiving instance's txid (set by DEFAULT).

`watermark` is the oldest still-in-progress transaction ID at commit time.
Everything with `local_version` below the watermark is guaranteed committed.

`local_version` is local bookkeeping — used for query ordering on the serving side but never transmitted.
`watermark` is transmitted in `PullResponse` so the receiver knows where to resume, but it is not part of the journal entry itself.

### Topological ordering via transaction IDs

Under REPEATABLE READ or higher isolation, a transaction can only read data committed before its snapshot was taken.
Committed data has a strictly lower transaction ID than the reading transaction.
Therefore, if entry B references entry A, then `A.local_version < B.local_version` on the instance where B was written.

This means the `local_version` ordering is topologically sorted for locally-created entries.
Replicated entries are committed before any local entry can reference them, so the combined `local_version` ordering across all origins is also topologically sorted.

When two replication streams run concurrently, each stream is independently topologically sorted.
Interleaving them by concurrent commits preserves topological ordering: if entry Y references entry X, then X must have been committed (and assigned a `local_version`) before Y's transaction could read it.

### At-least-once delivery via watermark

Entries may become visible out of `local_version` order because transactions can commit in a different order than they started.
If transaction T1 starts before T2 but T2 commits first, T2's entry may be served before T1's.

The `watermark` column ensures T1's entry is not skipped.
The serving loop winds the query position back to `batch.last().watermark` rather than advancing to `batch.last().local_version`.
A bounded seen buffer filters entries already sent in the current session.

### Serving pseudocode

```text
watermark = receiver's after_watermark (0 initially)
seen = bounded set of recently-served (origin_instance_id, origin_id, version) tuples

loop:
    batch = query entries WHERE local_version >= watermark
                            AND project_id = project
                          ORDER BY local_version
                          FETCH FIRST N ROWS WITH TIES

    if batch is empty:
        wait for notification or timeout
        continue

    for entry in batch:
        key = (entry.origin_instance_id, entry.origin_id, entry.version)
        if key not in seen:
            send PullResponse {
                watermark: entry.watermark,
                entry: entry
            }
            add key to seen

    watermark = batch.last().watermark
```

`WITH TIES` is the same-`local_version` atomicity from the batch-boundaries section above.

The seen buffer is bounded.
Entries can be evicted from the buffer once the watermark advances past their `local_version` — they will not appear in future queries.

### Batch boundaries

A `Pull` may stream entries across multiple batches.
For correctness, **a child entry must never be sent in a batch that strictly precedes its parent's batch**.
Otherwise a receiver applying batches in arrival order would briefly hold a child without its parent.

Two ancestor relationships count as parent-child:

- `previous_version` — same-resource chain on `JournalEntry`.
- Cross-resource references such as a comment's parent, transmitted in the resource-typed payload.

A receiver pulling from a relay can encounter same-`local_version` parent-child pairs, because a relay assigns the same receive-side `local_version` to all entries committed in one replicating transaction.
A naive batch boundary inside such a group splits the chain.

The protocol contract: items crossing batch boundaries are topologically ordered.
Implementations have two valid strategies for same-`local_version` groups:

- **Atomic same-`local_version` groups.**
  Every batch includes all rows sharing the last row's `local_version`.
  In SQL this is `LIMIT N WITH TIES` semantics — after the cut, fetch any remaining rows whose `local_version` matches the last row's.

- **Topo-sort within a `local_version`.**
  When a same-`local_version` group is split across batches, order rows within the group so parents precede children.
  Requires the sender to know in-batch parent/child relationships.

The first strategy is simpler and matches Postgres's `WITH TIES` clause directly.
References that resolve to entries embargoed-out of the receiver's view do not need to be in any batch — the embargo gap is permitted by the protocol.

### Receiving pseudocode

```text
watermark = load stored watermark for (upstream_id, project_id), default 0

stream = upstream.Pull(project_id, after_watermark=watermark)

for response in stream:
    key = (entry.origin_instance_id, entry.origin_id, entry.version)

    begin transaction (repeatable read)
        if key already exists locally:
            skip (dedup)
        else:
            insert entry into appropriate journal table
            (local_version and watermark are set automatically by DEFAULT)
        update stored watermark to max(watermark, response.watermark)
    commit

    # previous_version gaps are normal:
    # embargo filtering, thin clones, and partial replication
    # all produce chains where previous_version points to an
    # entry the receiver does not have.
    # The chain is informational, not a hard constraint.
```

### Multi-hop ordering proof

Consider instances A, B, C where C replicates from B, and B replicates bidirectionally with A.

1. A's entries are topologically sorted by A's `local_version` ordering (see above).
2. B receives A's entries and stores them.
   B's local entries may reference A's entries.
   Since A's entries are committed on B before B's referencing entries, B's `local_version` ordering is topologically sorted across both origins.
3. C replicates from B.
   B's stream to C is ordered by B's `local_version`, which is topologically sorted.
   C commits entries in stream order.
   C's `local_version` ordering is topologically sorted.

When B replicates back to A, B filters out entries that originated at A (A already has them).
Every entry from B that references an A-originated entry is topologically ordered with respect to A's entries, so A can commit them without dangling references.

### Isolation level enforcement

```sql
CREATE FUNCTION check_repeatable_read() RETURNS void AS $$
BEGIN
  IF current_setting('transaction_isolation') NOT IN
     ('repeatable read', 'serializable') THEN
    RAISE EXCEPTION
      'journal writes require repeatable read or higher isolation';
  END IF;
END;
$$ LANGUAGE plpgsql;
```

This function is called by write operations on journal tables to enforce the snapshot isolation requirement.
