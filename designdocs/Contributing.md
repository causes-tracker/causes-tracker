# Contributing to Causes

## Overall process

The design docs in `designdocs/` are the source of truth for architecture decisions.

Submit changes as GitHub pull requests.
Decisions that affect the architecture should be recorded in [Decisions.md](Decisions.md) as a new ADR, or as an update to an existing one.

For significant design changes, open an issue first to discuss the approach before writing a PR.

## Development environment

The repository uses [Bazel](https://bazel.build) as its build system.
Bazel is hermetic: it manages all toolchains itself.
**Do not use native tooling (`cargo`, `rustc`, `psql`, `yamllint`, etc.) directly — use the Bazel-wrapped equivalents documented below.**
Even within Bazel, most native tools are not wired up; only narrow cases like lockfile generation are supported.

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/) (for integration tests)
- [Bazel](https://bazel.build/install) (or the `bazelisk` wrapper included in the devcontainer)

### First-time setup

The repository ships a jj repo-level config at `tools/jj-repo-config.toml`.
jj does not yet support versioning files inside `.jj/` directly
(see [jj managed config design](https://docs.jj-vcs.dev/latest/design/managed-config/) for future plans),
so the file lives in `tools/` and must be symlinked into place once after cloning:

```sh
ln -s ../../tools/jj-repo-config.toml .jj/repo/config.toml
```

This registers merge drivers for `Cargo.lock` and `MODULE.bazel.lock` that regenerate those files automatically on `jj resolve --all`, instead of leaving conflict markers.

## Building and testing

Build everything:

```sh
bazel build //...
```

Run all tests (including lint checks):

```sh
bazel test //...
```

## Formatting

To format all source files in-place:

```sh
bazel run //:format
```

To check formatting without making changes (what CI does):

```sh
bazel run //:format.check
```

All formatters are hermetic: they run the exact pinned versions downloaded by Bazel.
Do not rely on any formatter installed in the devcontainer.

### Manually enforced formatting rules

All `.md` files use **sentence-per-line** formatting: one sentence per line, blank lines between paragraphs.
This keeps diffs small and reviewable.

## Version control (jj)

We use [Jujutsu (`jj`)](https://github.com/jj-vcs/jj) for local version control.
jj layers transparently over Git, so GitHub, CI, and the merge queue are unaffected.

See `CLAUDE.md` at the repository root for the full jj workflow including branching, stacking, and the merge-of-all-work pattern.

## CI

All merges go through the merge queue.
The `build` job (`bazel test //...`) must pass before a PR can merge.

To merge a PR, add the `merge` label.
A GitHub Action queues it into the merge queue when it targets master.
For stacked PRs, add the label to every PR in the stack — each one queues automatically when GitHub retargets it to master.
Never use the merge button directly.

## Infrastructure

Infrastructure is managed via OpenTofu through the Bazel wrapper `bazel run //infra:tofu -- <module> <args>`.
There are two root modules:

- `infra/terraform` — AWS infrastructure (Aurora, EC2, S3, networking).
  See `infra/terraform/README.md`.
- `infra/github` — GitHub repository settings, branch rulesets, and merge queue.
  See `infra/github/README.md`.

## Commit discipline

Each commit must do exactly one thing.
Commits must pass all linting and tests before being pushed.
Keep commits small: **400–500 diff lines maximum** (added + removed).
For larger changes, split into a sequence of focused commits.

Keep subject lines under 50 characters; put detail in the body.

## Getting help

Open an issue on GitHub to ask questions or start a design discussion.
