# jj snapshots are per-workspace

`jj` snapshots working-copy edits only in the workspace it is invoked from.
Disk edits in a secondary workspace (e.g. `~/causes-buck2`) stay invisible to jj commands run in the main workspace: a cross-workspace `jj squash --from <ws>@` moves the stale recorded content (possibly empty), and `jj workspace update-stale` resets the secondary working copy, destroying the unsnapshotted edits.

**Why:** fixes edited on disk in the buck2 workspace were squashed as empty from the main workspace; the unfixed commit reached CI, and the disk edits were then wiped by `update-stale` (PR #392, 2026-07-15).

**How to apply:** after editing files in a secondary workspace, run any snapshotting command (`jj st`) inside that workspace before referencing its `<ws>@` commit from anywhere else.
