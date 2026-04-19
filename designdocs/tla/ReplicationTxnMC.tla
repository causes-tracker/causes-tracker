--------------------------- MODULE ReplicationTxnMC ---------------------------
EXTENDS ReplicationTxn

MCNodes == {"A", "B", "C"}
MCProjects == {"P1"}
MCResourceIds == {"r1"}
MCTrust == [
    n \in MCNodes |->
        IF n = "A" THEN {"B"}
        ELSE IF n = "B" THEN {"A"}
        ELSE {}
]
MCMaxOps == 9

=============================================================================
