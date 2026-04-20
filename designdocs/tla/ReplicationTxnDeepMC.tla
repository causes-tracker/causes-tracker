--------------------------- MODULE ReplicationTxnDeepMC ---------------------------
(* Deeper model-checking run.  Tagged manual so it doesn't run in every CI *)
(* cycle; run with                                                         *)
(* `bazel test //designdocs/tla:replication_txn_deep_tlc_test`.            *)
EXTENDS ReplicationTxn

CONSTANTS NodeA, NodeB, NodeC

MCNodes == {NodeA, NodeB, NodeC}
MCProjects == {"P1"}
MCResourceIds == {"r1"}
MCTrust == [
    n \in MCNodes |->
        IF n = NodeA THEN {NodeB}
        ELSE IF n = NodeB THEN {NodeA}
        ELSE {}
]
MCMaxOps == 10

\* {NodeA, NodeB} are interchangeable (Trust is symmetric across them);
\* NodeC is asymmetric (untrusted by both).  Cuts state space ~2x.
MCSymmetry == Permutations({NodeA, NodeB})

=============================================================================
