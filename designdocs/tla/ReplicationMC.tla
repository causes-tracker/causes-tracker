---------------------------- MODULE ReplicationMC ----------------------------
(***************************************************************************)
(* Model-checking configuration for Replication.tla.  Binds constants to   *)
(* concrete values; TLC's .cfg file can't express record literals directly *)
(* so we define them here and substitute via CONSTANTS <- in the .cfg.     *)
(***************************************************************************)
EXTENDS Replication

CONSTANTS NodeA, NodeB, NodeC

MCNodes == {NodeA, NodeB, NodeC}
MCProjects == {"P1"}
MCResourceIds == {"r1"}

\* A and B mutually trust each other for embargoed content; C is excluded.
MCTrust == [
    n \in MCNodes |->
        IF n = NodeA THEN {NodeB}
        ELSE IF n = NodeB THEN {NodeA}
        ELSE {}
]

MCMaxOps == 6

\* {NodeA, NodeB} are interchangeable (Trust is symmetric across them);
\* NodeC is asymmetric (untrusted by both).  Cuts state space ~2x.
MCSymmetry == Permutations({NodeA, NodeB})

=============================================================================
