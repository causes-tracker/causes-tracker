# Causes — Claude instructions

## Markdown style

All `.md` files use **sentence-per-line** formatting:
one sentence per line, blank lines between paragraphs.
This keeps diffs small and reviewable.
List item continuation sentences are indented to align with the list marker.

## Commit discipline

Each commit must do exactly one thing.
Commits must pass all linting and tests before being made.
Commits are kept strictly small: **400–500 lines maximum** (diff lines added + removed).
If a change is larger, split it into a sequence of focused commits.

For `bazel run` targets (servers, binaries): run them and confirm they start correctly before pushing — even long-running ones, which should be started, verified, then killed.
"It builds" is not the same as "it works."

## Git workflow

We use [Jujutsu (`jj`)](https://github.com/jj-vcs/jj) for local version control.
`jj` layers over Git transparently — GitHub, CI, and the merge queue are unaffected.

The master branch is protected: the `build` CI job must pass and all merges go through the merge queue (Merge mode).

**Key concepts:**

- `@` is the working copy change — edits are tracked automatically, no staging area.
- Bookmarks are `jj`'s name for Git branches.
- `jj undo` safely reverses any operation.

**Starting new work:**

```sh
jj git fetch
jj new master -m "what you're doing"
# edit files — changes are part of @ immediately
jj describe -m "final message"        # refine when ready
```

**Opening a PR:**

```sh
jj git push --named <name>=@
gh pr create --base <parent-bookmark> ...
```

The PR base branch must match the jj parent change's bookmark.
If the change is a direct child of `master@origin`, the base is `master`.
If it stacks on another change, the base is that change's bookmark.
Never rebase a change to fix a wrong PR base — use `gh pr edit <n> --base <bookmark>` instead.

**Keeping a branch up to date with master (never merge):**

```sh
jj git fetch
jj rebase -d master
jj git push -b <name> --force-with-lease
```

**After PRs merge — rebase and tidy the merge-of-all-work:**

```sh
jj git fetch && jj rebase -r 'mutable()' -d master@origin
jj simplify-parents -r <merge-change>   # drop parents now reachable via master
```

**Working with multiple changes in parallel (merge-of-all-work pattern):**

Keep a merge change that combines all in-progress branch tips, and a scratch
change after it as the working copy:

```sh
jj new a b c -m "temp: merge all in-progress"   # create the merge
jj new @                                          # scratch change after it
```

To promote the scratch change into a real branch and add it to the merge:

```sh
jj rebase -r <scratch> -A <intended-parent> -B <merge-change>
# -A sets the new parent (old parent is not preserved); -B inserts it before the merge
jj git push --named <name>=<scratch>
# After the rebase, @ moves to the promoted change — restore the scratch position:
jj new <merge-change>
```

After `jj rebase -r <scratch> -A ... -B <merge>`, the working copy (`@`) lands
on the promoted change (now inside the merge parents), not after the merge.
Always run `jj new <merge-change>` immediately after to restore `@` to the
scratch position above the merge.

Always work on a new change at the tip; use `jj squash --into <target>` to
move it to the right place in history once it works.

**Graph surgery — moving a single node:**

The key principle: identify the one node that is in the wrong place and move only it.
Never try to fix a cascade by rebasing multiple nodes in sequence — that causes conflicts.

```sh
# Move node X to be a child of Y, inserting it before Z:
jj rebase -r <X> -A <Y> -B <Z>

# Move node X to be a direct child of master (independent branch):
jj rebase -r <X> -A master -B <merge-change>

# Add an existing node into the merge-of-all-work without changing its parent:
jj rebase -r <node> -A <its-current-parent> -B <merge-change>
```

**Resolving conflicts:**

Never abandon a conflicted commit and start over.
Instead: create a child of the conflicted commit, write the correct content, then squash down.

```sh
jj new <conflicted-change>   # work on top of it
# edit files to the correct resolved state
jj squash                    # fold the resolution back into the conflicted commit
```

**`gh` commands always need explicit branches:**

`gh` does not understand jj workspaces and cannot infer the current branch.
Always pass `--base` and `--head` explicitly.

```sh
gh pr create --base master --head <bookmark-name> ...
gh pr edit <n> --base <bookmark-name>
```

**Useful commands:**

```sh
jj log       # graph of changes
jj diff      # what changed in @
jj squash    # fold @ into its parent
jj abandon 'empty() & ~merges() & mutable()'   # safely drop all empty scratch changes
```
