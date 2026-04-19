---------------------------- MODULE ReplicationMC ----------------------------
(***************************************************************************)
(* Model-checking configuration for Replication.tla.  Binds constants to   *)
(* concrete values; TLC's .cfg file can't express record literals directly *)
(* so we define them here and substitute via CONSTANTS <- in the .cfg.     *)
(***************************************************************************)
EXTENDS Replication

MCNodes == {"A", "B", "C"}
MCProjects == {"P1"}
MCResourceIds == {"r1"}

\* A and B mutually trust each other for embargoed content; C is excluded.
MCTrust == [
    n \in MCNodes |->
        IF n = "A" THEN {"B"}
        ELSE IF n = "B" THEN {"A"}
        ELSE {}
]

MCMaxOps == 6

=============================================================================
