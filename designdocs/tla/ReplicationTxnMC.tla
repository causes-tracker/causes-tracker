--------------------------- MODULE ReplicationTxnMC ---------------------------
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
MCMaxOps == 9

\* {NodeA, NodeB} are interchangeable (Trust is symmetric across them);
\* NodeC is asymmetric (untrusted by both).  Cuts state space ~2x.
MCSymmetry == Permutations({NodeA, NodeB})

=============================================================================
