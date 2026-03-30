# Causes — Claude instructions

## Security

Always apply least privilege, even in MVPs and dev environments.
Never suggest broad permissions (e.g. `AdministratorAccess`, `*` resource) as a shortcut.
Scope IAM policies, security groups, and credentials to exactly what is needed.
If a managed policy has gaps, add scoped inline statements — do not escalate to a wider policy.

## Markdown style

All `.md` files use **sentence-per-line** formatting:
one sentence per line, blank lines between paragraphs.
This keeps diffs small and reviewable.
List item continuation sentences are indented to align with the list marker.

## Build system

This project uses [Bazel](https://bazel.build) exclusively.
**Never use native tooling (`cargo`, `rustc`, `psql`, `tofu`, `terraform`, `yamllint`, etc.) directly.**
Use the Bazel-wrapped equivalents:

```sh
bazel build //...                     # build everything
bazel test //...                      # run all tests + lint
bazel run //:format                   # format all source files in-place
bazel run //:format.check             # check formatting without changes (what CI runs)
bazel run //infra/postgres:psql -- …  # hermetic psql
bazel run //infra:tofu -- …           # hermetic OpenTofu (auto-cds into infra/terraform)
bazel run //tools:sqlx -- …           # hermetic sqlx-cli
bazel run //tools:cargo -- …          # hermetic cargo (metadata only, not compilation)
```

Lint checks are Bazel test targets included in `//...`.
There is no separate lint command.

## Incremental development

When building a feature incrementally across multiple commits, do not expose new **user-facing** interfaces until the feature is ready.
CLI flags, environment variables, `--help` text, API endpoints, and documentation should not be visible to users until the implementing code lands in the same or a prior commit.
Internal code (traits, modules, functions, DB schema) can land before its callers — that is normal incremental development.
Use `#[cfg]` attributes, feature flags, or simply defer adding the user-facing interface to the commit that adds the implementation.

## Commit discipline

Each commit must do exactly one thing.
Commits must pass all linting and tests before being made.
Commits are kept strictly small: **400–500 lines maximum** (diff lines added, ignoring generated files, ignoring removed lines).
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

Always work in scratch — the empty change sitting above the merge-of-all-work change.
Edit files there; they are tracked automatically.
When ready, promote the scratch into a real branch:

```sh
jj describe -m "what you did"                          # name the scratch
jj rebase -r @ -A master -B <merge-change>             # promote: child of master, before merge
jj git push --named <name>=@                           # first push — creates the bookmark
jj new <merge-change>                                  # restore scratch position
```

**Pushing to GitHub:**

Always push with `jj git push --all` — never push individual bookmarks.
A bookmark points at a jj changeset, not a git revision.
When a changeset evolves (via squash, rebase, etc.) the bookmark follows automatically — there is no need to move it.
`jj git push --named` is only for creating a brand-new bookmark on first push; never use it for a bookmark that already exists.

**Opening a PR:**

```sh
jj git push --named <name>=@   # first push only — creates the bookmark
jj git push --all               # all subsequent pushes
gh pr create --base <parent-bookmark> --head <name> ...
```

The PR base branch must match the jj parent change's bookmark.
If the change is a direct child of `master`, the base is `master`.
If it stacks on another change, the base is that change's bookmark.
Never rebase a change to fix a wrong PR base — use `gh pr edit <n> --base <bookmark>` instead.

**Keeping branches up to date with master (never merge):**

Rebase the entire working set — not individual branches — to avoid missing anything:

```sh
jj git fetch
jj rebase -r 'mutable()' -d master
jj git push --all
```

**After PRs merge — rebase and tidy the merge-of-all-work:**

```sh
jj git fetch && jj rebase -r 'mutable()' -d master
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
jj git push --named <name>=<scratch>   # first push — creates the bookmark
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
gh pr create --base master --head <bookmark-name> --title "..." --body "..."
gh pr edit <n> --base <bookmark-name>
```

**Lockfile merge drivers — one-time setup after clone:**

`tools/jj-repo-config.toml` contains merge drivers that auto-regenerate
`Cargo.lock` and `MODULE.bazel.lock` instead of leaving conflict markers.
jj cannot version files inside `.jj/`, so the file lives in `tools/` and
must be symlinked once after cloning:

```sh
ln -s ../../tools/jj-repo-config.toml .jj/repo/config.toml
```

After this, `jj resolve --all` on a conflicted change regenerates the lockfiles
automatically via `bazel run //tools:cargo -- generate-lockfile` and `bazel mod tidy`.

**Useful commands:**

```sh
jj log       # graph of changes
jj diff      # what changed in @
jj squash    # fold @ into its parent
jj abandon 'empty() & ~merges() & mutable()'   # safely drop all empty scratch changes
```
