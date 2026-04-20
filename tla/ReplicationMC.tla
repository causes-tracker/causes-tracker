--------------------------- MODULE ReplicationMC ---------------------------
EXTENDS Replication

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
MCMaxOps == 7
\* BatchSize=2 stresses partial batches without exploding state space.
\* Same-localVersion plan+comment groups (size 2) just barely fit;
\* larger same-V groups would force topo-sorted within-V splits.
MCBatchSize == 2

\* {NodeA, NodeB} are interchangeable (Trust is symmetric across them);
\* NodeC is asymmetric (untrusted by both).  Cuts state space ~2x.
MCSymmetry == Permutations({NodeA, NodeB})

=============================================================================
