# Plans are ephemeral; only facts go in the repo

In-flight plans, status, and "next steps" do not belong in the source repo (including `.claude/rules/project/`): branches jump around, so an in-repo plan changes under you and is stale the moment work lands.

**Why:** PR #390 (a buck2 status+plan doc under rules/project) was closed for this.

**How to apply:** classify such content into exactly one of:
(a) real documentation — follow project standards, shipped with the feature it documents;
(b) durable non-obvious facts — a memory, committed under `.claude/rules/` and merged;
(c) work-in-progress sequencing — the ephemeral session plan file, accepting it dies with the container.
