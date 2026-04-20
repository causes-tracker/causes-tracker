---------------------------- MODULE Replication ----------------------------
(***************************************************************************)
(* TLA+ specification of the Causes replication protocol.                  *)
(*                                                                         *)
(* Models a small bounded state space: a few nodes, one project, one       *)
(* resource id, a handful of operations.  Despite the small size, the      *)
(* state space exercises the interesting behaviours: dedup under multi-    *)
(* path delivery, embargo filtering with asymmetric trust, the previous-   *)
(* version chain across instances, and replication path tracking.          *)
(*                                                                         *)
(* Out-of-order commits (the watermark mechanism) are NOT modelled here    *)
(* — we treat each write as atomic and version monotonic per node.  The    *)
(* watermark simulation lives in the Rust `txn_sim` module instead.        *)
(*                                                                         *)
(* Run:                                                                    *)
(*   designdocs/tla/run_tlc.sh                                             *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    Nodes,         \* Set of node identifiers, e.g. {"A", "B", "C"}
    Projects,      \* Set of projects, e.g. {"P1"}
    ResourceIds,   \* Set of resource identifiers, e.g. {"r1"}
    Trust,         \* Function: Node -> SUBSET Nodes (peers served embargoed)
    MaxOps         \* Bound on total operations (keeps state space finite)

ASSUME Trust \in [Nodes -> SUBSET Nodes]
ASSUME MaxOps \in Nat

VARIABLES
    entries,        \* Function: Node -> SUBSET Entry
    cursors,        \* Function: Node -> Node -> Project -> Nat
    nextVersion,    \* Function: Node -> Nat
    opCount         \* Total number of operations performed

vars == <<entries, cursors, nextVersion, opCount>>

(***************************************************************************)
(* An entry carries the journal header fields plus path tracking:          *)
(*   origin: the writing node (origin_instance_id)                         *)
(*   rid:    the resource id (origin_id)                                   *)
(*   ver:    the version assigned by the writing node                     *)
(*   prev:   federated reference to previous version (or NullRef)         *)
(*   emb:    embargo flag                                                  *)
(*   proj:   the project this entry belongs to                            *)
(*   path:   sequence of nodes — first is origin, last is storing node    *)
(***************************************************************************)
NullRef == [origin |-> "_", rid |-> "_", ver |-> 0]

VRef(e) == [origin |-> e.origin, rid |-> e.rid, ver |-> e.ver]

\* Has the receiver already stored an entry with this federated identity?
Has(receiver, origin, rid, ver) ==
    \E e \in entries[receiver] :
        e.origin = origin /\ e.rid = rid /\ e.ver = ver

\* Maximum of a non-empty natural-number set
Max(S) == CHOOSE x \in S : \A y \in S : x >= y

(***************************************************************************)
(* Initial state: nothing exists, all cursors zero, no ops performed.     *)
(***************************************************************************)
Init ==
    /\ entries = [n \in Nodes |-> {}]
    /\ cursors = [n \in Nodes |-> [u \in Nodes |-> [p \in Projects |-> 0]]]
    /\ nextVersion = [n \in Nodes |-> 1]
    /\ opCount = 0

(***************************************************************************)
(* Action: a node creates a brand-new resource.                            *)
(* Constraint: this node has not previously created an entry with this    *)
(* (origin=self, rid) pair.                                               *)
(***************************************************************************)
Create(n, rid, proj, emb) ==
    /\ opCount < MaxOps
    /\ \A e \in entries[n] : ~(e.origin = n /\ e.rid = rid)
    /\ LET v == nextVersion[n]
           entry == [origin |-> n, rid |-> rid, ver |-> v,
                     prev   |-> NullRef, emb |-> emb,
                     proj   |-> proj, path |-> <<n>>]
       IN
           /\ entries' = [entries EXCEPT ![n] = @ \cup {entry}]
           /\ nextVersion' = [nextVersion EXCEPT ![n] = v + 1]
    /\ opCount' = opCount + 1
    /\ UNCHANGED cursors

(***************************************************************************)
(* Action: a node edits an entry it already has.  Produces a new entry    *)
(* on the editing node; previous_version chains back to the source.       *)
(* The new entry's origin is the editor (cross-instance edits create new  *)
(* origin entries; same-instance edits keep origin = editor too).         *)
(***************************************************************************)
Edit(n, prev) ==
    /\ opCount < MaxOps
    /\ prev \in entries[n]
    /\ LET v == nextVersion[n]
           entry == [origin |-> n, rid |-> prev.rid, ver |-> v,
                     prev   |-> VRef(prev), emb |-> prev.emb,
                     proj   |-> prev.proj, path |-> <<n>>]
       IN
           /\ entries' = [entries EXCEPT ![n] = @ \cup {entry}]
           /\ nextVersion' = [nextVersion EXCEPT ![n] = v + 1]
    /\ opCount' = opCount + 1
    /\ UNCHANGED cursors

(***************************************************************************)
(* Action: receiver pulls all eligible entries from upstream for proj.    *)
(* Filters: not originated at receiver; embargoed only if upstream serves  *)
(* embargo to receiver; not already present (dedup).                      *)
(* Cursor advances to the max version of all visible entries (since we    *)
(* don't model out-of-order commits, version itself is the watermark).    *)
(***************************************************************************)
Pull(receiver, upstream, proj) ==
    /\ opCount < MaxOps
    /\ receiver # upstream
    /\ LET cur        == cursors[receiver][upstream][proj]
           serveEmb   == receiver \in Trust[upstream]
           visible    == { e \in entries[upstream] :
                             e.proj = proj /\ e.ver >= cur }
           candidates == { e \in visible :
                             /\ e.origin # receiver
                             /\ (~e.emb \/ serveEmb)
                             /\ ~Has(receiver, e.origin, e.rid, e.ver) }
           accepted   == { [origin |-> e.origin, rid |-> e.rid, ver |-> e.ver,
                            prev   |-> e.prev, emb |-> e.emb,
                            proj   |-> e.proj,
                            path   |-> e.path \o <<receiver>>] :
                           e \in candidates }
           newCursor  == IF visible = {} THEN cur ELSE Max({e.ver : e \in visible})
       IN
           /\ entries' = [entries EXCEPT ![receiver] = @ \cup accepted]
           /\ cursors' =
                 [cursors EXCEPT ![receiver] =
                    [@ EXCEPT ![upstream] =
                       [@ EXCEPT ![proj] = newCursor]]]
    /\ opCount' = opCount + 1
    /\ UNCHANGED nextVersion

(***************************************************************************)
(* Next-state relation: at each step, perform one of the available        *)
(* actions.  Quantifiers enumerate every possible parameter combination.  *)
(***************************************************************************)
Next ==
    \/ \E n \in Nodes, rid \in ResourceIds, proj \in Projects, emb \in BOOLEAN :
        Create(n, rid, proj, emb)
    \/ \E n \in Nodes : \E e \in entries[n] : Edit(n, e)
    \/ \E r \in Nodes, u \in Nodes, p \in Projects : Pull(r, u, p)

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(*                              Invariants                                 *)
(***************************************************************************)

\* No two entries on a node share federated identity.
NoDuplicates ==
    \A n \in Nodes :
        \A e1, e2 \in entries[n] :
            (e1.origin = e2.origin /\ e1.rid = e2.rid /\ e1.ver = e2.ver)
                => e1 = e2

\* Path begins with the writing instance.
PathStartsWithOrigin ==
    \A n \in Nodes :
        \A e \in entries[n] :
            Head(e.path) = e.origin

\* Path ends with the storing node.
PathEndsWithSelf ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.path[Len(e.path)] = n

\* An embargoed entry has a path consisting only of trusted hops:
\* every step path[i] -> path[i+1] is allowed because path[i] serves
\* embargoed content to path[i+1].
EmbargoFilter ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.emb =>
                \A i \in 1..(Len(e.path) - 1) :
                    e.path[i+1] \in Trust[e.path[i]]

\* Every entry's project is one of the configured projects.
ProjectIsolation ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.proj \in Projects

\* If an entry's previous_version is non-null, the referenced entry is
\* either present on the same node OR is embargoed (acceptable gap when
\* we couldn't legally see it).
ChainIntactOrEmbargoGap ==
    \A n \in Nodes :
        \A e \in entries[n] :
            e.prev # NullRef =>
                \/ Has(n, e.prev.origin, e.prev.rid, e.prev.ver)
                \/ \E other \in Nodes :
                       \E p \in entries[other] :
                           /\ p.origin = e.prev.origin
                           /\ p.rid = e.prev.rid
                           /\ p.ver = e.prev.ver
                           /\ p.emb

\* Type correctness — useful sanity check, also bounds the state space.
TypeOK ==
    /\ entries \in [Nodes -> SUBSET [
            origin: Nodes \cup {"_"},
            rid: ResourceIds \cup {"_"},
            ver: 0..MaxOps,
            prev: [origin: Nodes \cup {"_"}, rid: ResourceIds \cup {"_"}, ver: 0..MaxOps],
            emb: BOOLEAN,
            proj: Projects,
            path: Seq(Nodes)
        ]]
    /\ cursors \in [Nodes -> [Nodes -> [Projects -> 0..MaxOps]]]
    /\ nextVersion \in [Nodes -> 1..(MaxOps + 1)]
    /\ opCount \in 0..MaxOps

=============================================================================
