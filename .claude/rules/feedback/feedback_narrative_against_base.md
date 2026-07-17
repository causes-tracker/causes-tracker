# Narrative reads against the base branch, never prior iterations

Code comments, commit messages, and PR descriptions must make sense to a reader diffing against the base branch.
References to earlier iterations of the same unmerged changeset — "no longer X", "what remains", "instead of the previous approach" — describe states that never exist in history once the changeset lands.

**Why:** on PR #409 a test comment explained what the test no longer asserted relative to a prior push of the same branch; the same pattern had been rejected before.

**How to apply:** when revising an unmerged changeset, rewrite affected comments, commit messages, and PR bodies from scratch against the base.
Describe what the code is, not the edit that produced it.
This is a specific case to check during the review-0 pass ([[feedback_review_0_before_push]]).
