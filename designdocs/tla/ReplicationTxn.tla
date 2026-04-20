--------------------------- MODULE ReplicationTxn ---------------------------
(***************************************************************************)
(* Protocol-faithful TLA+ model: admits any implementation that satisfies  *)
(* the causal-ordering constraint on versions, including those (like the   *)
(* Postgres reference implementation) where transactions can commit out    *)
(* of order.                                                               *)
(*                                                                         *)
(* Differences from Replication.tla:                                       *)
(* - Each insert begins a transaction with a fresh version, then commits   *)
(*   later (possibly after other transactions have committed).             *)
(* - Each row carries a watermark = min(in-flight versions at insert       *)
(*   time).  This is the "safe resume point" for cursor wind-back.         *)
(* - Pull only sees committed rows.                                        *)
(* - Cursor advances to the max watermark of visible entries (not max     *)
(*   version), allowing the protocol to deliver late-committing entries   *)
(*   that have ver < cursor on a subsequent pull.                          *)
(*                                                                         *)
(* This subsumes the simpler model: any execution of Replication.tla      *)
(* corresponds to a serialised execution here (where every Begin is       *)
(* immediately followed by Commit before any other action).                *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS Nodes, Projects, ResourceIds, Trust, MaxOps, BatchSize

ASSUME Trust \in [Nodes -> SUBSET Nodes]
ASSUME MaxOps \in Nat
ASSUME BatchSize \in Nat \ {0}

VARIABLES
    entries,        \* Node -> SUBSET Entry
    cursors,        \* Node -> Node -> Project -> Nat
    nextVersion,    \* Node -> Nat (txid allocator)
    inFlight,       \* Node -> SUBSET Nat (in-flight txids)
    opCount

vars == <<entries, cursors, nextVersion, inFlight, opCount>>

NullRef == [origin |-> "_", rid |-> "_", ver |-> 0]
VRef(e) == [origin |-> e.origin, rid |-> e.rid, ver |-> e.ver]

Min(S) == CHOOSE x \in S : \A y \in S : x <= y
Max(S) == CHOOSE x \in S : \A y \in S : x >= y

\* xmin observed at insert time on node n: the lowest in-flight version
\* (including the new one being allocated).
XminWith(n, newV) == Min(inFlight[n] \cup {newV})

Has(receiver, origin, rid, ver) ==
    \E e \in entries[receiver] :
        e.origin = origin /\ e.rid = rid /\ e.ver = ver

Init ==
    /\ entries = [n \in Nodes |-> {}]
    /\ cursors = [n \in Nodes |-> [u \in Nodes |-> [p \in Projects |-> 0]]]
    /\ nextVersion = [n \in Nodes |-> 1]
    /\ inFlight = [n \in Nodes |-> {}]
    /\ opCount = 0

(***************************************************************************)
(* Action: open a transaction and write a brand-new resource entry        *)
(* (plan-style: no parent reference).                                     *)
(*                                                                         *)
(* For locally-authored entries: localVersion == ver == txid.             *)
(***************************************************************************)
BeginNewPlan(n, rid, proj, emb) ==
    /\ opCount < MaxOps
    /\ \A e \in entries[n] : ~(e.origin = n /\ e.rid = rid)
    /\ LET v == nextVersion[n]
           wm == XminWith(n, v)
           entry == [origin |-> n, rid |-> rid, ver |-> v,
                     localVersion |-> v, watermark |-> wm,
                     prev |-> NullRef, parentRef |-> NullRef,
                     emb |-> emb,
                     committed |-> FALSE,
                     proj |-> proj, path |-> <<n>>]
       IN
           /\ entries' = [entries EXCEPT ![n] = @ \cup {entry}]
           /\ nextVersion' = [nextVersion EXCEPT ![n] = v + 1]
           /\ inFlight' = [inFlight EXCEPT ![n] = @ \cup {v}]
    /\ opCount' = opCount + 1
    /\ UNCHANGED cursors

(***************************************************************************)
(* Action: open a transaction and write a comment-style entry that        *)
(* references a parent plan.  The parent must be committed and present    *)
(* on this node (you can't reference what you can't see).                 *)
(***************************************************************************)
BeginNewComment(n, rid, proj, parent, emb) ==
    /\ opCount < MaxOps
    /\ \A e \in entries[n] : ~(e.origin = n /\ e.rid = rid)
    /\ parent \in entries[n]
    /\ parent.committed
    /\ parent.parentRef = NullRef    \* parent is itself a plan (not a comment)
    /\ parent.proj = proj            \* parent in same project
    /\ LET v == nextVersion[n]
           wm == XminWith(n, v)
           entry == [origin |-> n, rid |-> rid, ver |-> v,
                     localVersion |-> v, watermark |-> wm,
                     prev |-> NullRef, parentRef |-> VRef(parent),
                     emb |-> emb,
                     committed |-> FALSE,
                     proj |-> proj, path |-> <<n>>]
       IN
           /\ entries' = [entries EXCEPT ![n] = @ \cup {entry}]
           /\ nextVersion' = [nextVersion EXCEPT ![n] = v + 1]
           /\ inFlight' = [inFlight EXCEPT ![n] = @ \cup {v}]
    /\ opCount' = opCount + 1
    /\ UNCHANGED cursors

(***************************************************************************)
(* Action: write a comment-style entry into an EXISTING open transaction  *)
(* whose first entry was a plan.  Models Postgres read-your-writes within *)
(* a transaction: the comment can reference its parent before the parent  *)
(* is committed, because both will commit atomically together.            *)
(*                                                                         *)
(* The new comment shares ver and localVersion with the parent (same      *)
(* transaction = same txid).                                              *)
(***************************************************************************)
WriteCommentInOpenPlanTxn(n, parent, rid, emb) ==
    /\ opCount < MaxOps
    /\ parent \in entries[n]
    /\ parent.parentRef = NullRef    \* parent is a plan
    /\ ~parent.committed             \* parent's txn still open
    /\ parent.ver \in inFlight[n]
    /\ \A e \in entries[n] : ~(e.origin = n /\ e.rid = rid)
    /\ LET txid == parent.ver
           wm == XminWith(n, txid)
           entry == [origin |-> n, rid |-> rid, ver |-> txid,
                     localVersion |-> txid, watermark |-> wm,
                     prev |-> NullRef, parentRef |-> VRef(parent),
                     emb |-> emb,
                     committed |-> FALSE,
                     proj |-> parent.proj, path |-> <<n>>]
       IN entries' = [entries EXCEPT ![n] = @ \cup {entry}]
    /\ opCount' = opCount + 1
    /\ UNCHANGED <<cursors, nextVersion, inFlight>>

(***************************************************************************)
(* Action: open a transaction that edits an existing committed entry.     *)
(* Same-instance edit pattern.  Preserves the parentRef of the previous.  *)
(***************************************************************************)
BeginEdit(n, prev) ==
    /\ opCount < MaxOps
    /\ prev \in entries[n]
    /\ prev.committed
    /\ LET v == nextVersion[n]
           wm == XminWith(n, v)
           entry == [origin |-> n, rid |-> prev.rid, ver |-> v,
                     localVersion |-> v, watermark |-> wm,
                     prev |-> VRef(prev), parentRef |-> prev.parentRef,
                     emb |-> prev.emb,
                     committed |-> FALSE,
                     proj |-> prev.proj, path |-> <<n>>]
       IN
           /\ entries' = [entries EXCEPT ![n] = @ \cup {entry}]
           /\ nextVersion' = [nextVersion EXCEPT ![n] = v + 1]
           /\ inFlight' = [inFlight EXCEPT ![n] = @ \cup {v}]
    /\ opCount' = opCount + 1
    /\ UNCHANGED cursors

(***************************************************************************)
(* Action: commit an in-flight transaction.  All entries with version=v   *)
(* on this node become visible.                                           *)
(***************************************************************************)
Commit(n, v) ==
    /\ opCount < MaxOps
    /\ v \in inFlight[n]
    /\ entries' = [entries EXCEPT ![n] =
            { IF e.ver = v
              THEN [e EXCEPT !.committed = TRUE]
              ELSE e : e \in entries[n] }]
    /\ inFlight' = [inFlight EXCEPT ![n] = @ \ {v}]
    /\ opCount' = opCount + 1
    /\ UNCHANGED <<cursors, nextVersion>>

(***************************************************************************)
(* Validity of a batch.  A batch is the subset of visible entries selected *)
(* by one Pull RPC; subsequent Pulls fetch the rest.  Two constraints:     *)
(*                                                                         *)
(*   1. PREFIX: batch is a prefix in localVersion order.  Equivalent to    *)
(*      the Postgres `ORDER BY local_version LIMIT N` query semantics.     *)
(*      Required for cursor advance (max watermark of batch) to never      *)
(*      skip a committed entry — see NoCommittedEntryLost.                 *)
(*                                                                         *)
(*   2. ANCESTOR-CLOSED: for every entry e in batch and every ancestor    *)
(*      reference r in {e.prev, e.parentRef} that resolves to a visible    *)
(*      entry q, q is in batch or already at the receiver.  Without this   *)
(*      the receiver could briefly hold an entry whose ancestor (prev for  *)
(*      same-resource chain, parentRef for cross-resource) is in a         *)
(*      strictly later batch.                                              *)
(*                                                                         *)
(*      Note: multiple entries can share localVersion on a receiver — when *)
(*      a batch is pulled and committed in one txn, all entries get the    *)
(*      same recvTxid.  Two such entries could have a prev/parent link     *)
(*      between them, so same-localVersion groups must respect the         *)
(*      ancestor closure as well.                                          *)
(*                                                                         *)
(* Implementations satisfy (2) by either shipping same-localVersion        *)
(* groups atomically (LIMIT N WITH TIES) or topo-sorting within a group.   *)
(***************************************************************************)
ValidBatch(receiver, upstream, proj, batch) ==
    LET cur     == cursors[receiver][upstream][proj]
        visible == { e \in entries[upstream] :
                       /\ e.committed
                       /\ e.proj = proj
                       /\ e.localVersion >= cur }
    IN /\ batch # {}
       /\ batch \subseteq visible
       /\ Cardinality(batch) <= BatchSize
       /\ \A e \in batch : \A f \in visible :
            f.localVersion < e.localVersion => f \in batch
       /\ \A e \in batch :
            \A ref \in {e.prev, e.parentRef} \ {NullRef} :
              \A p \in visible :
                (p.origin = ref.origin /\ p.rid = ref.rid /\ p.ver = ref.ver) =>
                  \/ p \in batch
                  \/ Has(receiver, p.origin, p.rid, p.ver)

(***************************************************************************)
(* Action: receiver pulls a batch of committed entries from upstream.     *)
(* Filter is on `localVersion` (upstream's local commit order), not `ver` *)
(* (origin's version).  The receiver assigns its own localVersion and    *)
(* watermark when storing the entry — replicated entries get a fresh txid *)
(* in the receiver's namespace, matching the reference implementation.   *)
(* Cursor advances to max(watermark) of the batch; subsequent pulls pick *)
(* up entries whose localVersion >= that watermark.                      *)
(***************************************************************************)
BatchPull(receiver, upstream, proj, batch) ==
    /\ opCount < MaxOps
    /\ receiver # upstream
    /\ ValidBatch(receiver, upstream, proj, batch)
    /\ LET serveEmb   == receiver \in Trust[upstream]
           candidates == { e \in batch :
                             /\ e.origin # receiver
                             /\ (~e.emb \/ serveEmb)
                             /\ ~Has(receiver, e.origin, e.rid, e.ver) }
           recvTxid   == nextVersion[receiver]
           recvWm     == XminWith(receiver, recvTxid)
           accepted   == { [origin |-> e.origin, rid |-> e.rid, ver |-> e.ver,
                            localVersion |-> recvTxid, watermark |-> recvWm,
                            prev |-> e.prev, parentRef |-> e.parentRef,
                            emb |-> e.emb,
                            committed |-> TRUE,
                            proj |-> e.proj,
                            path |-> e.path \o <<receiver>>] :
                           e \in candidates }
           newCursor  == Max({e.watermark : e \in batch})
       IN
           /\ entries' = [entries EXCEPT ![receiver] = @ \cup accepted]
           /\ cursors' = [cursors EXCEPT ![receiver] =
                            [@ EXCEPT ![upstream] =
                               [@ EXCEPT ![proj] = newCursor]]]
           /\ nextVersion' = IF candidates = {} THEN nextVersion
                             ELSE [nextVersion EXCEPT ![receiver] = recvTxid + 1]
    /\ opCount' = opCount + 1
    /\ UNCHANGED inFlight

Next ==
    \/ \E n \in Nodes, rid \in ResourceIds, proj \in Projects, emb \in BOOLEAN :
        BeginNewPlan(n, rid, proj, emb)
    \/ \E n \in Nodes, rid \in ResourceIds, proj \in Projects, emb \in BOOLEAN :
        \E parent \in entries[n] : BeginNewComment(n, rid, proj, parent, emb)
    \/ \E n \in Nodes, rid \in ResourceIds, emb \in BOOLEAN :
        \E parent \in entries[n] : WriteCommentInOpenPlanTxn(n, parent, rid, emb)
    \/ \E n \in Nodes : \E e \in entries[n] : BeginEdit(n, e)
    \/ \E n \in Nodes : \E v \in inFlight[n] : Commit(n, v)
    \/ \E r \in Nodes, u \in Nodes, p \in Projects :
        \E batch \in SUBSET entries[u] : BatchPull(r, u, p, batch)

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(*                              Invariants                                 *)
(***************************************************************************)

NoDuplicates ==
    \A n \in Nodes :
        \A e1, e2 \in entries[n] :
            (e1.origin = e2.origin /\ e1.rid = e2.rid /\ e1.ver = e2.ver)
                => e1 = e2

PathStartsWithOrigin ==
    \A n \in Nodes :
        \A e \in entries[n] :
            Head(e.path) = e.origin

PathEndsWithSelf ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.path[Len(e.path)] = n

EmbargoFilter ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.emb =>
                \A i \in 1..(Len(e.path) - 1) :
                    e.path[i+1] \in Trust[e.path[i]]

ProjectIsolation ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.proj \in Projects

\* The new, more interesting invariant.  If the cursor on r for (u, p) has
\* advanced past some committed entry e on u, then either r has e, or r
\* would never legally receive e (origin or embargo filter).
\*
\* In other words: no committed entry is permanently lost just because the
\* cursor moved past it.  This is precisely what the watermark mechanism
\* exists to prevent — without it, a cursor advance to max(version) could
\* skip a late-committing low-version entry.
NoCommittedEntryLost ==
    \A r \in Nodes :
        \A u \in Nodes :
            \A p \in Projects :
                \A e \in entries[u] :
                    (e.committed /\ e.proj = p
                        /\ e.localVersion < cursors[r][u][p]) =>
                            \/ Has(r, e.origin, e.rid, e.ver)
                            \/ e.origin = r
                            \/ (e.emb /\ r \notin Trust[u])

\* previous_version chain integrity (only checked on committed entries).
ChainIntactOrEmbargoGap ==
    \A n \in Nodes :
        \A e \in entries[n] :
            (e.committed /\ e.prev # NullRef) =>
                \/ \E q \in entries[n] :
                    /\ q.committed
                    /\ q.origin = e.prev.origin
                    /\ q.rid = e.prev.rid
                    /\ q.ver = e.prev.ver
                \/ \E other \in Nodes : \E p \in entries[other] :
                    /\ p.origin = e.prev.origin
                    /\ p.rid = e.prev.rid
                    /\ p.ver = e.prev.ver
                    /\ p.emb

\* Cross-resource references resolve locally OR have a known embargo gap.
\* If a comment-style entry refers to a parent plan, that parent must be on
\* the same node, OR be embargoed (acceptable gap when we couldn't legally
\* see it).  The protocol's topological-ordering claim is what's being
\* verified here: a relay that delivers a comment must have the parent.
ParentRefIntactOrEmbargoGap ==
    \A n \in Nodes :
        \A e \in entries[n] :
            (e.committed /\ e.parentRef # NullRef) =>
                \/ \E q \in entries[n] :
                    /\ q.committed
                    /\ q.origin = e.parentRef.origin
                    /\ q.rid = e.parentRef.rid
                    /\ q.ver = e.parentRef.ver
                \/ \E other \in Nodes : \E p \in entries[other] :
                    /\ p.origin = e.parentRef.origin
                    /\ p.rid = e.parentRef.rid
                    /\ p.ver = e.parentRef.ver
                    /\ p.emb

TypeOK ==
    /\ entries \in [Nodes -> SUBSET [
            origin: Nodes \cup {"_"},
            rid: ResourceIds \cup {"_"},
            ver: 0..(MaxOps * Cardinality(Nodes)),
            localVersion: 0..(MaxOps * Cardinality(Nodes)),
            watermark: 0..(MaxOps * Cardinality(Nodes)),
            prev: [origin: Nodes \cup {"_"},
                   rid: ResourceIds \cup {"_"},
                   ver: 0..(MaxOps * Cardinality(Nodes))],
            parentRef: [origin: Nodes \cup {"_"},
                        rid: ResourceIds \cup {"_"},
                        ver: 0..(MaxOps * Cardinality(Nodes))],
            emb: BOOLEAN,
            committed: BOOLEAN,
            proj: Projects,
            path: Seq(Nodes)
        ]]
    /\ cursors \in [Nodes -> [Nodes -> [Projects -> 0..(MaxOps * Cardinality(Nodes))]]]
    /\ nextVersion \in [Nodes -> 1..(MaxOps + 1)]
    /\ inFlight \in [Nodes -> SUBSET (1..MaxOps)]
    /\ opCount \in 0..MaxOps

=============================================================================
