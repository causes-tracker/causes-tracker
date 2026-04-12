---
name: jj
description: >-
  This skill should be used when performing version control operations,
  creating changesets, pushing code, creating branches or bookmarks, rebasing,
  resolving conflicts, running "jj" or "git" commands, creating PRs, or any
  task involving version control.
  Also triggered by "/jj" for on-demand jj guidance.
  This project uses Jujutsu (jj) — never run raw git commands.
user-invocable: true
allowed-tools: [Bash, Read, Grep, Glob]
---

# Jujutsu (jj) version control

This project uses Jujutsu (`jj`) for all version control.
Never run raw `git` commands — they corrupt jj's operation log.
All git interaction goes through `jj git fetch` and `jj git push`.

## Pre-push checklist (mandatory after any rebase/reorder)

Before every `jj git push --all` after graph changes:

1. `jj log` — are bookmarks where you expect them?
2. Any open PR whose **base branch is moving** in this push?
   → `gh pr edit <N> --base master` to temp-base it first.
   (The risk: if a PR's head becomes reachable from its base after the push,
   GitHub falsely marks it "merged" — code never reaches master.)
3. `jj git push --all`
4. Fix PR bases back: `gh pr edit <N> --base <correct-branch>`
5. Verify bijection: `gh pr list --json number,headRefName,baseRefName`

Skip this checklist only for routine pushes with no rebase/reorder.

## Critical rules

1. **No raw git.** Never run `git commit`, `git push`, `git checkout`, etc.
   Use `jj describe`, `jj git push`, `jj new`, etc.

2. **No staging area.** `@` (the working copy) IS the commit.
   Edits are tracked automatically.
   There is no `jj add` or staged/unstaged distinction.

3. **`jj new` before every new task.** Before editing any file for a new piece
   of work, run `jj log` and confirm `@` is the right changeset.
   If it isn't: `jj new master` (new work) or `jj edit <target>` (existing work).
   **This is the single most common mistake — editing files on the wrong
   changeset and then needing graph surgery to fix it.**
   After finishing a change, run `jj new` to create a fresh empty changeset.
   This protects the completed work from accidental edits.

4. **Always push with `--all`.** Editing any commit in a stack changes the
   hashes of all descendants — their bookmarks must be updated on the remote
   too, or PRs will show stale content.
   To create a new bookmark and push the whole stack:
   `jj bookmark set <name> -r @` then `jj git push --all`.
   (`--all` and `--named` cannot be combined.)
   For subsequent pushes: `jj git push --all`.
   Never use `--named` alone — it only pushes the one bookmark.

5. **`gh` needs explicit flags.** `gh` cannot infer branches from jj.
   Always pass `--base` and `--head` to `gh pr create` and `gh pr edit`.

6. **`jj log` needs no flags.** It shows only mutable changes by default.
   Do not pass `--limit` or `-n` — these are git idioms, not jj.

7. **Bookmarks auto-follow rewrites.** After squash/rebase, the bookmark tracks
   the change ID automatically.
   Never manually run `jj bookmark set` after a rewrite.

8. **Commit messages: max 50-char subject.** Use `jj describe -m "subject"` for
   simple messages.
   For longer messages with a body, use `jj describe` (opens editor) or
   pass a multi-line string.

## Stacked vs merge-all: choosing the right PR shape

Before creating PRs from multiple commits, decide: **stacked** or **merge-all**?

**Stacked (linear chain):** Each PR targets the previous PR's branch.
Use when commits are **sequential** — commit B's diff only makes sense applied
after commit A (e.g. both edit the same file, or B calls a function A introduces).

```
master ← A ← B ← C       (PR-A base=master, PR-B base=branch-A, ...)
```

**Merge-all (independent siblings):** Each PR targets master directly.
Use when commits are **independent** — they touch different files/areas and
their diffs apply cleanly in any order.

```
master ← A
       ← B                (PR-A base=master, PR-B base=master, ...)
       ← C
```

**The decision criterion is semantic, not textual.**
Ask: "does it make sense to merge B into master without A?"
If B calls a function A introduces, or extends behaviour A defines, they must
be stacked — even if the diffs touch different files and rebase cleanly.
If A and B are genuinely independent features that make sense in either order,
they should be siblings.

## This repo's workflow

The master branch is protected: the `build` CI job must pass and all merges
go through the merge queue.

### Merge-of-all-work pattern

Keep a merge change combining all in-progress branch tips, with a scratch
change after it as the working copy:

```sh
jj new a b c -m "temp: merge all in-progress"   # create the merge
jj new @                                          # scratch on top
```

### Promote scratch to a real branch

```sh
jj describe -m "what you did"
jj rebase -r @ -A master -B <merge-change>
jj git push --named <name>=@
jj new <merge-change>                            # MUST restore scratch position
```

**After `jj rebase -r @ -A ... -B <merge>`, `@` lands on the promoted change
(now inside the merge parents), NOT after the merge.**
Always run `jj new <merge-change>` immediately after.

### Opening a PR

```sh
jj git push --named <name>=@                     # first push only
gh pr create --base master --head <name> --title "..." --body "..."
```

The base branch must match the jj parent's bookmark.
Direct child of master -> base is `master`.
Stacked on another change -> base is that change's bookmark.
To fix a wrong base: `gh pr edit <n> --base <correct-bookmark>` (never rebase).

### Keeping up to date (never merge, always rebase)

```sh
jj git fetch
jj rebase -r 'mutable()' -d master
jj git push --all
```

### After PRs merge

```sh
jj git fetch && jj rebase -r 'mutable()' -d master
jj simplify-parents -r <merge-change>
```

### Graph surgery — moving a single node

Identify the one node in the wrong place and move only it.
Never cascade-rebase multiple nodes — that causes conflicts.

```sh
jj rebase -r <X> -A <Y> -B <Z>           # X becomes child of Y, before Z
jj rebase -r <X> -A master -B <merge>    # independent branch off master
```

### Safe reorder with open PRs

When reordering commits with open PRs, GitHub can falsely mark a PR as
"merged" if its head becomes reachable from its base branch after the push.
The PR closes (purple) but code never reaches master.

**How it happens:** PR-B has `--base branch-A --head branch-B`.
You reorder so B's commit becomes an ancestor of A's, then push both.
GitHub sees B's head reachable from A and concludes B was merged into A.

**Follow the pre-push checklist at the top of this document.**

### Resolving conflicts

Never abandon a conflicted commit.
Create a child, resolve, squash back down:

```sh
jj new <conflicted-change>
# edit files to the correct resolved state
jj squash
```

### Lockfile merge drivers

`tools/jj-repo-config.toml` must be symlinked once after clone:

```sh
ln -s ../../tools/jj-repo-config.toml .jj/repo/config.toml
```

After this, `jj resolve --all` auto-regenerates `Cargo.lock` and
`MODULE.bazel.lock`.

## Quick command reference

| Instead of (git)          | Use (jj)                                    |
|---------------------------|---------------------------------------------|
| `git add` + `git commit`  | Just edit files; `jj describe -m "..."`     |
| `git commit --amend`      | Edit files (auto-tracked); or `jj squash`   |
| `git log`                 | `jj log` (no flags needed)                  |
| `git diff`                | `jj diff`                                   |
| `git diff --staged`       | N/A — no staging area                       |
| `git stash`               | `jj new` (start a new change)               |
| `git checkout <branch>`   | `jj edit <rev>`                             |
| `git branch -d`           | `jj abandon` (rebases descendants)          |
| `git push`                | `jj git push --all`                         |
| `git fetch`               | `jj git fetch`                              |
| `git rebase`              | `jj rebase` (see reference for flag details)|
| `git reflog` + reset      | `jj undo`                                   |
| `git restore <file>`      | `jj restore <file>`                         |

## Additional resources

For detailed jj knowledge, rebase flag reference, revset cheat sheet, and
common git-brain mistakes with corrections, consult:

- **`references/jj-for-claude.md`** — comprehensive jj reference for Claude
