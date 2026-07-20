# Review 0: self-review the whole changeset before every push

Before any push — and before asking the user to look — review the entire changeset as a diff against its **base branch**, hunk by hunk, stricter than the user would be.
Do it as a fresh reader: a subagent that never saw the intermediate drafts is ideal, because anything referencing a concept absent from the base then stands out.
Give that reader the user's requirements as they stated them, quoted — not your paraphrase — so it can flag anything not asked for or contradicting the goal; a diff-vs-base review alone grades whatever was built as long as it is internally correct.
This pass runs before the push, never after; a finding surfaced after the push has already cost the user time the pass exists to save.

**Why:** across PR #409 and its successors, every correction the user made was catchable here — residue of intermediate edit states, narrative written against prior drafts, still-accurate comments dropped in a rewrite, and tautological tests.

The checklist below is the reviewer's mandate.
Each item is a defect to fix or justify before pushing.

## 1. Every changed line earns its place against the base

Flag reordering of existing items with no functional reason.
Flag a wholesale rewrite where a surgical edit would do — text you did not mean to change must stay byte-identical.
Flag a rename of an already-adequate name.
Flag a helper or wrapper that only adds indirection: a single caller, a one-line delegation, or a seam that exists only for a test.
Flag any hunk a reviewer would meet with "why is this in the diff?".

## 2. Removed lines audited for lost signal, not just added lines for noise

For every deleted comment or doc line, it must have gone false or redundant.
A still-accurate, non-obvious comment dropped during a rewrite is a regression — the surrounding change being legitimate is not cover.

## 3. Narrative reads against the base, never prior drafts of this branch

Comments, docs, the commit message, and the PR body describe what the code IS versus the base.
Flag "no longer", "instead of", "previously", "no default", "avoids X", "rather than", and any reference to a concept not present in the base.

## 4. Comments earn their place

Flag any comment that restates the adjacent code, an assertion, or the function's own docstring; a comment is justified only by a non-obvious why.
Prose and doc comments use one sentence per line — a sentence may wrap, two sentences on one line may not.

## 5. Tests prove their claim

The test must drive the actual production value or path, not a proxy that may not be wired in.
No tautological assertion: the asserted observable must distinguish the case under test from every other case that yields the same value (a `0` or `None` that also means "ran and found nothing" fails this).
Distinct outcomes must be distinct values or types, not one overloaded sentinel.
A plausible broken implementation must fail some test.

## 6. Design integrity — challenge, don't rubber-stamp

Flag a concern placed in a generic or shared crate that belongs to a specific consumer.
Flag public API or visibility wider than the callers require.
Flag a vague or inaccurate name.
Flag a "for now" / YAGNI shape chosen to avoid the clean design.
Flag a type default or sentinel that permits a loose, ambiguous instance.

## 7. Stack and commit hygiene

The base's real state must be verified (fetched) before building on it or claiming a stack relationship; a merged PR is in master, not a base to stack on.
One commit, one concern — an unrelated edit does not ride along.

Iterating on review feedback raises the risk rather than lowering it: every accepted suggestion leaves residue shaped like the previous structure, so re-run the whole pass after each round, against the base.
