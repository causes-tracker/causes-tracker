# Never rewrite history another workspace sits on

Rewriting a commit from one jj workspace (`squash --into`, `rebase`, `describe`) rebases all of its descendants, including the other workspace's working-copy commit, and leaves that workspace stale.
A stale workspace fails every jj command that snapshots, so hooks running there fail, and `jj workspace update-stale` resets its working copy.

**Why:** squashing fixes into a stacked changeset from `~/causes-buck2` while `/workspaces/causes` sat on the stack made the main workspace stale mid-session (2026-10-19); the Stop hook could not compute `master..@` until the user ran `update-stale`.

**How to apply:** before rewriting a changeset, move every other workspace whose `@` descends from it onto unrelated history (`jj new master` there), rewrite, then move it back (`jj new <changeset>`).
If the other workspace cannot be moved, do not rewrite; leave the edit in a new commit on top.
[[project_jj_workspace_snapshots]] covers the other direction: snapshot in the workspace that holds the edits before referencing them elsewhere.
