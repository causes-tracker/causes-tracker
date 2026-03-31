# Jujutsu (jj) reference for Claude

## Mental model: jj is not git

| Git concept               | jj equivalent                | Key difference                                      |
|---------------------------|------------------------------|-----------------------------------------------------|
| `git add` + `git commit`  | Edit files; `jj describe`    | No staging area; `@` is always a commit             |
| Commit hash (SHA)         | Commit ID + Change ID        | Change ID is stable across rewrites                 |
| Branch pointer            | Bookmark                     | Auto-follows when change is rewritten               |
| HEAD                      | `@` (working copy)           | Always a real commit, never detached                |
| Index / staging area      | Does not exist               | All edits auto-tracked in `@`                       |
| `git stash`               | `jj new` (new empty change)  | Just start a new change; old one is preserved       |
| `git checkout <branch>`   | `jj edit <rev>`              | Completely different command                         |
| `git reflog` + `git reset`| `jj undo` / `jj op restore`  | Operation-level undo, clean and predictable         |
| Merge conflict (blocking) | Conflict in commit            | Non-blocking; work continues                        |

## The working copy (`@`)

`@` is the working copy commit.
It is a real commit that is automatically updated every time any jj command runs.
There is no separate "staged" vs "unstaged" state.

- `@` — current working copy commit
- `@-` — parent of working copy
- `@--` — grandparent

`jj diff` shows what changed in `@` vs its parent.
`jj status` shows working-copy changes and any conflicts.
New files are tracked automatically on next snapshot — no `jj add` needed.

To split a change into pieces, use `jj split` or `jj diffedit`, not
a staging workflow.

## Common git-brain mistakes

### 1. Running raw git commands

Never run `git commit`, `git push`, `git checkout`, `git branch`, etc.
Raw git commands bypass jj's operation log and can corrupt state.
All git interaction goes through `jj git fetch` and `jj git push`.

### 2. Looking for a staging area

There is no `jj add`.
There is no staged vs unstaged.
Edits in the working directory ARE the commit `@`.
`jj diff` is like `git diff HEAD`, not `git diff --staged`.

### 3. Using `--limit` or `-n` with `jj log`

`jj log` shows only mutable changes by default — not the entire repo history.
`--limit` is not a jj flag.
Use revsets to filter: `jj log -r 'trunk()..@'`.

### 4. Manually moving bookmarks after rewrite

Bookmarks track change IDs, not commit hashes.
When a change is rewritten (squash, rebase, describe), the bookmark follows
automatically.
Never run `jj bookmark set` after a rewrite — it is unnecessary.

### 5. Using `--named` for existing bookmarks

`jj git push --named <name>=<rev>` creates a brand-new bookmark.
For existing bookmarks, use `jj git push --all`.
Using `--named` on an existing bookmark creates a conflict.

### 6. Looking for `jj commit`

There is no commit command.
The workflow is: edit files (auto-tracked in `@`), `jj describe -m "message"`,
then `jj new` to start the next change.

### 7. Forgetting to restore scratch position after rebase

After `jj rebase -r @ -A <parent> -B <merge>`, the working copy lands on
the promoted change (now a parent of the merge), NOT after the merge.
Always run `jj new <merge-change>` immediately after to restore the scratch
position.

### 8. Abandoning conflicted commits

`jj abandon` on a conflicted commit throws away the conflict state and rebases
descendants onto the pre-conflict parent.
Instead: `jj new <conflicted>`, edit to the correct state, `jj squash`.

### 9. Using `-s` when `-r` is needed for graph surgery

`-r` moves ONLY the named revision (descendants fill the gap).
`-s` moves the revision AND all its descendants.
For single-node graph surgery, always use `-r`.

### 10. Running `jj config set` unnecessarily

The jj editor and user identity are already configured in this devcontainer.
Do not run `jj config set --user ui.editor` or similar — it is already done.

## Rebase guide

### Which revisions to move (mutually exclusive)

| Flag     | What moves                              | Descendants               |
|----------|-----------------------------------------|---------------------------|
| `-r`     | Only the named revision                 | Fill the hole (re-parent) |
| `-s`     | The revision AND all descendants        | Move together             |
| `-b`     | The whole branch (roots of `dest..rev`) | Move together             |

Default (no flag): `-b @`.

### Where to put them

| Flag          | Effect                                                   |
|---------------|----------------------------------------------------------|
| `-d` / `-o`   | Rebase onto target; existing children of target unaffected |
| `-A` / `--after` | Insert after target; target's children rebase onto new revs |
| `-B` / `--before`| Insert before target; target rebases onto new revs        |

`-A` and `-B` can be combined in one command.

### Diagrams

**`jj rebase -r K -A L`** (insert K after L, slide L's children up):

```
N           N'
|           |
| M         | M'
|/          |/
L      =>   K'
|           |
| K         L
|/          |
J           J
```

**`jj rebase -r K -B L`** (insert K before L):

```
L     =>    L'
|           |
| K         K'
|/          |
J           J
```

**`jj rebase -s M -d O`** (move M and descendants onto O):

```
O           N'
|           |
| N         M'
| |         |
| M    =>   O
```

## Revset cheat sheet

### Symbols

- `@` — working copy
- `root()` — repo root commit
- `<bookmark-name>` — tip of that bookmark
- `<change-id-prefix>` — unique prefix of a change ID
- `<commit-id-prefix>` — unique prefix of a commit ID

### Operators (strongest binding first)

| Operator  | Meaning                                            |
|-----------|----------------------------------------------------|
| `x-`      | Parents of x                                       |
| `x+`      | Children of x                                      |
| `x::`     | x and all its descendants                          |
| `::x`     | x and all its ancestors                            |
| `x::y`    | Ancestry path: descendants of x that are ancestors of y |
| `x..y`    | Ancestors of y NOT ancestors of x (like git `x..y`) |
| `~x`      | Complement (everything not in x)                   |
| `x & y`   | Intersection                                       |
| `x ~ y`   | Difference (x minus y)                             |
| `x \| y`  | Union                                              |

### Common functions

| Function            | Meaning                                        |
|---------------------|-------------------------------------------------|
| `mutable()`         | Commits safe to rewrite (not in trunk/tags)    |
| `immutable_heads()` | Heads of immutable set (trunk, tags, untracked)|
| `empty()`           | Commits with no file changes vs parent         |
| `merges()`          | Commits with >1 parent                         |
| `heads(x)`          | Head commits of revset x                       |
| `roots(x)`          | Root commits of revset x                       |
| `trunk()`           | Latest immutable ancestor of the working copy  |

### Useful patterns

```
mutable()                           # all your in-progress work
empty() & ~merges() & mutable()     # empty scratch changes (safe to abandon)
trunk()..@                          # your work since branching from trunk
```

## Bookmark lifecycle

1. **Create + first push:** `jj git push --named <name>=@`
2. **Subsequent pushes:** `jj git push --all`
3. **Auto-follows rewrites:** squash, rebase, describe — bookmark moves automatically
4. **Delete:** `jj bookmark delete <name>`, then push to propagate
5. **Forget (local only):** `jj bookmark forget <name>` — does not mark as deleted on remote
6. **Track remote:** `jj bookmark track <name>@<remote>`

## Conflict resolution

Conflicts in jj are non-blocking.
They are recorded in the commit itself.
The repo keeps working — rebase, push, and other operations proceed normally.

### Resolution pattern

```sh
jj new <conflicted-change>   # create a child
# edit files to the correct resolved state
jj squash                    # fold resolution back into the conflicted commit
```

Never `jj abandon` a conflicted commit — that throws away the conflict state.

### Lockfile conflicts

With the merge drivers in `tools/jj-repo-config.toml` (must be symlinked to
`.jj/repo/config.toml`), `jj resolve --all` auto-regenerates `Cargo.lock`
and `MODULE.bazel.lock`.

## `jj undo` — operation-level reversal

Every jj mutation is recorded in an operation log.
`jj undo` cleanly reverses the last operation — not just a ref log lookup.

```sh
jj undo                  # reverse the last operation
jj op log                # view the operation history
jj op restore <op-id>    # jump to a specific operation state
```

`jj undo` can be called repeatedly to go further back.

## Useful shortcuts

- `jj new <parent>` — create a new change and move `@` there in one step.
  Never use `jj new --no-edit` + `jj edit` separately.
- `jj squash --from X --into Y` — move changes between any two revisions,
  not just child-into-parent.
- `jj squash -u` — keep the destination's description (discard the source's).
  Use this or `-m` to avoid opening an editor.
- `jj abandon 'empty() & ~merges() & mutable()'` — safely drop all empty
  scratch changes.
