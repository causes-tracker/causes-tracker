# Never move the shell's working directory

The Stop and pre-push hooks run `tools/check.sh` in the shell's current directory.
A `cd` into the buck2 workspace persists across Bash calls, so the next hook run executes Bazel inside the buck2 checkout, which starts a Bazel server there and writes `bazel-*` symlinks that break buck2.

**Why:** two stray Bazel servers and a set of `bazel-*` symlinks appeared in `~/causes-buck2` after commands that began with `cd ~/causes-buck2` (2026-11-22); the end-of-turn gate then failed `format_check` over the wip spike tree instead of the main workspace.

**How to apply:** run commands in the other workspace from a subshell, `(cd ~/causes-buck2 && tools/buck2/buck2.sh …)`, or with `jj -R ~/causes-buck2 …`.
The shell itself stays in `/workspaces/causes`.
