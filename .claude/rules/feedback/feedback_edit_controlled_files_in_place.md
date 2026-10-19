# Edit controlled files in place, not via new-then-squash

An authorisation token for a quality-gate file hashes the file as it is in `@-` of the workspace the hooks run in.
Editing on a fresh child (`jj new <changeset>`) and squashing the edit down moves it into `@-`, so the base content changes and the token stops matching.

**Why:** a `tools/check.sh` edit was authorised while it sat in the working copy and unauthorised the moment it was squashed into its changeset (2026-10-19).

**How to apply:** for a controlled file, `jj edit <changeset>` and make the edit there, so `@-` stays the changeset's parent for the whole hour.
New-then-squash remains the pattern for uncontrolled fixes in a stack, where there is no token to keep.
Controlled files are listed by `buck2 run //tools:authorise -- --list`.
