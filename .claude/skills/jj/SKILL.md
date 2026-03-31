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

## Critical rules

1. **No raw git.** Never run `git commit`, `git push`, `git checkout`, etc.
   Use `jj describe`, `jj git push`, `jj new`, etc.

2. **No staging area.** `@` (the working copy) IS the commit.
   Edits are tracked automatically.
   There is no `jj add` or staged/unstaged distinction.

3. **Fresh changeset per session.** Run `jj new master` (or the appropriate parent)
   before starting any new work.
   Never edit in an existing changeset left over from prior work.

4. **`--named` is only for first push.** To create a new bookmark and push:
   `jj git push --named <name>=@`.
   For all subsequent pushes: `jj git push --all`.
   Never use `--named` for a bookmark that already exists.

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
