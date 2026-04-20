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

CONSTANTS Nodes, Projects, ResourceIds, Trust, MaxOps

ASSUME Trust \in [Nodes -> SUBSET Nodes]
ASSUME MaxOps \in Nat

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
(* Action: open a transaction and write a brand-new resource entry.       *)
(* The entry is uncommitted; visible only after Commit.                   *)
(*                                                                         *)
(* For locally-authored entries: localVersion == ver == txid.             *)
(***************************************************************************)
BeginNew(n, rid, proj, emb) ==
    /\ opCount < MaxOps
    /\ \A e \in entries[n] : ~(e.origin = n /\ e.rid = rid)
    /\ LET v == nextVersion[n]
           wm == XminWith(n, v)
           entry == [origin |-> n, rid |-> rid, ver |-> v,
                     localVersion |-> v, watermark |-> wm,
                     prev |-> NullRef, emb |-> emb,
                     committed |-> FALSE,
                     proj |-> proj, path |-> <<n>>]
       IN
           /\ entries' = [entries EXCEPT ![n] = @ \cup {entry}]
           /\ nextVersion' = [nextVersion EXCEPT ![n] = v + 1]
           /\ inFlight' = [inFlight EXCEPT ![n] = @ \cup {v}]
    /\ opCount' = opCount + 1
    /\ UNCHANGED cursors

(***************************************************************************)
(* Action: open a transaction that edits an existing committed entry.     *)
(* Same-instance edit pattern.                                            *)
(***************************************************************************)
BeginEdit(n, prev) ==
    /\ opCount < MaxOps
    /\ prev \in entries[n]
    /\ prev.committed
    /\ LET v == nextVersion[n]
           wm == XminWith(n, v)
           entry == [origin |-> n, rid |-> prev.rid, ver |-> v,
                     localVersion |-> v, watermark |-> wm,
                     prev |-> VRef(prev), emb |-> prev.emb,
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
(* Action: receiver pulls committed entries from upstream.                *)
(* Filter is on `localVersion` (upstream's local commit order), not `ver` *)
(* (origin's version).  The receiver assigns its own localVersion and    *)
(* watermark when storing the entry — replicated entries get a fresh txid *)
(* in the receiver's namespace, matching the reference implementation.   *)
(***************************************************************************)
Pull(receiver, upstream, proj) ==
    /\ opCount < MaxOps
    /\ receiver # upstream
    /\ LET cur        == cursors[receiver][upstream][proj]
           serveEmb   == receiver \in Trust[upstream]
           visible    == { e \in entries[upstream] :
                             /\ e.committed
                             /\ e.proj = proj
                             /\ e.localVersion >= cur }
           candidates == { e \in visible :
                             /\ e.origin # receiver
                             /\ (~e.emb \/ serveEmb)
                             /\ ~Has(receiver, e.origin, e.rid, e.ver) }
           recvTxid   == nextVersion[receiver]
           recvWm     == XminWith(receiver, recvTxid)
           accepted   == { [origin |-> e.origin, rid |-> e.rid, ver |-> e.ver,
                            localVersion |-> recvTxid, watermark |-> recvWm,
                            prev |-> e.prev, emb |-> e.emb,
                            committed |-> TRUE,
                            proj |-> e.proj,
                            path |-> e.path \o <<receiver>>] :
                           e \in candidates }
           newCursor  == IF visible = {} THEN cur
                         ELSE Max({e.watermark : e \in visible})
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
        BeginNew(n, rid, proj, emb)
    \/ \E n \in Nodes : \E e \in entries[n] : BeginEdit(n, e)
    \/ \E n \in Nodes : \E v \in inFlight[n] : Commit(n, v)
    \/ \E r \in Nodes, u \in Nodes, p \in Projects : Pull(r, u, p)

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
