--------------------------- MODULE ReplicationTxnDeepMC ---------------------------
(* Deeper model-checking run: MaxOps=8.  Tagged manual so it doesn't run   *)
(* in every CI cycle; run with                                             *)
(* `bazel test //designdocs/tla:replication_txn_deep_tlc_test`.            *)
EXTENDS ReplicationTxn

CONSTANTS NodeA, NodeB, NodeC

MCNodes == {NodeA, NodeB, NodeC}
MCProjects == {"P1"}
MCResourceIds == {"plan", "comment"}
MCTrust == [
    n \in MCNodes |->
        IF n = NodeA THEN {NodeB}
        ELSE IF n = NodeB THEN {NodeA}
        ELSE {}
]
MCMaxOps == 8
\* BatchSize=1 forces every batch to a single entry, maximally stressing
\* the parent-closure constraint: a child can only be shipped once its
\* parent is at the receiver.
MCBatchSize == 1

\* {NodeA, NodeB} are interchangeable (Trust is symmetric across them);
\* NodeC is asymmetric (untrusted by both).  Cuts state space ~2x.
MCSymmetry == Permutations({NodeA, NodeB})

=============================================================================
