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
bazel run //infra:tofu -- <module> …  # hermetic OpenTofu (e.g. infra/terraform, infra/github)
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

## Testing

Tests must never mutate global process state.
In particular, never call `std::env::set_var` in tests — it is unsound in multithreaded programs (Rust 2024 marks it `unsafe`) and causes flaky failures when tests run in parallel.
If code reads environment variables, refactor it to accept the value as a parameter so tests can pass it directly.

## Error handling

DB and infrastructure layer errors must be **typed**, not stringified.
Never inspect error messages with `.to_string().contains(...)` to decide control flow.
Instead, define domain-specific error enums (e.g. `ProjectError::NameAlreadyExists`) and catch the underlying error structurally (e.g. Postgres error code `23505` for unique violations).
The API layer pattern-matches on the typed error to choose the gRPC status code.

## Commit discipline

Each commit must do exactly one thing.
Commits must pass all linting and tests before being made.
Commits are kept strictly small: **400–500 lines maximum** (diff lines added, ignoring generated files, ignoring removed lines).
If a change is larger, split it into a sequence of focused commits.

Before pushing, verify the change the way CI does:
run `tools/coverage.sh //...` (not bare `bazel test //...`) — it runs coverage and enforces per-file thresholds.

For `bazel run` targets (servers, binaries): run them and confirm they start correctly before pushing — even long-running ones, which should be started, verified, then killed.
"It builds" is not the same as "it works."

## Communication style

Do not validate or assess the user's points ("You're right", "Good question", "Great point", etc.).
When the user makes a correction or observation, respond with substance — the edit, the counterpoint, the next step.

## Environment

Development happens in disposable containers.
Only save session-local memories (ephemeral, current-conversation-only) outside the repo.
Durable memories (feedback, preferences, project context) must go in-repo (CLAUDE.md, skill files, etc.) so they survive container destruction.

## Git workflow

This project uses [Jujutsu (`jj`)](https://github.com/jj-vcs/jj) for local version control.
See `.claude/skills/jj/` for complete jj workflow guidance.
