--------------------------- MODULE ReplicationTxnDeepMC ---------------------------
(* Deeper model-checking run: MaxOps=10.  Runs for ~17s and explores      *)
(* ~2.9M states.  Tagged manual so it doesn't run in every CI cycle;      *)
(* run with `bazel test //designdocs/tla:replication_txn_deep_tlc_test`.  *)
EXTENDS ReplicationTxn

MCNodes == {"A", "B", "C"}
MCProjects == {"P1"}
MCResourceIds == {"plan", "comment"}
MCTrust == [
    n \in MCNodes |->
        IF n = "A" THEN {"B"}
        ELSE IF n = "B" THEN {"A"}
        ELSE {}
]
MCMaxOps == 8

=============================================================================
