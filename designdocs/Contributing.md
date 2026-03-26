# Contributing to Causes

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

Run all tests (including lint):

```sh
bazel test //...
```

Some tests require a running PostgreSQL instance and are wrapped in a hermetic fixture that starts one automatically.
The integration test target `//services/causes_api:causes_api_db_test` uses the bundled PostgreSQL binary from `//infra/postgres:postgres_binaries`.

## Linting

All lint checks run as Bazel test targets and are included in `bazel test //...`.
There is no separate lint step.

| Language / format | Tool | Targets |
|---|---|---|
| Rust | [clippy](https://doc.rust-lang.org/clippy/) | `//services/causes_api:clippy` |
| Shell scripts | [shellcheck](https://www.shellcheck.net) | `//...` — every package with shell scripts has a `shellcheck` test target |
| YAML files | [yamllint](https://yamllint.readthedocs.io) | `//...` — every package with YAML files has a `yamllint` test target |
| Protocol Buffers | [buf lint](https://buf.build/docs/lint/overview/) | `//proto:buf_lint` |

To run only a specific package's lint checks:

```sh
bazel test //services/causes_api:shellcheck
bazel test //proto:buf_lint
bazel test //:yamllint
```

### Known gaps

- **Markdown linting**: no hermetic prebuilt binary exists for markdownlint-cli2; not yet integrated.
- **TOML schema validation**: taplo can validate against JSON Schema; not yet configured per-file.

## Formatting

To format all source files in-place:

```sh
bazel run //:format
```

To check formatting without making changes (what CI does):

```sh
bazel run //:format.check
```

| Language / format | Tool |
|---|---|
| Shell scripts | [shfmt](https://github.com/mvdan/sh) |
| YAML files | [yamlfmt](https://github.com/google/yamlfmt) |
| Starlark (BUILD files) | [buildifier](https://github.com/bazelbuild/buildtools/tree/main/buildifier) |
| TOML files | [taplo](https://taplo.tamasfe.dev) |
| Rust | [rustfmt](https://rust-lang.github.io/rustfmt/) |

All formatters are hermetic: they run the exact pinned versions downloaded by Bazel.
Do not rely on any formatter installed in the devcontainer.

## Hermetic tool wrappers

Several tools are exposed as `bazel run` targets for interactive use:

```sh
bazel run //infra/postgres:psql -- -U causes causes    # hermetic psql
bazel run //tools:sqlx -- migrate run                  # hermetic sqlx-cli
bazel run //tools:cargo -- generate-lockfile            # hermetic cargo (metadata only)
bazel run //tools:rustfmt -- src/main.rs               # hermetic rustfmt
```

These wrappers pin the tool version via Bazel and do not depend on anything installed in the devcontainer.
For compilation and testing, always use `bazel build` / `bazel test` rather than `cargo build` / `cargo test`.

## Working with the database

Start a local PostgreSQL instance with Docker Compose:

```sh
docker compose up postgres
```

Copy the example env file and fill in credentials:

```sh
cp services/causes_api/env.example services/causes_api/.env
```

The `DATABASE_URL` env var follows the format `postgresql://user:pass@host:port/dbname`.
Run migrations:

```sh
DATABASE_URL=postgresql://causes:causes@localhost:5432/causes \
  bazel run //tools:sqlx -- migrate run \
  --source services/causes_api/migrations
```

## Version control (jj)

We use [Jujutsu (`jj`)](https://github.com/jj-vcs/jj) for local version control.
jj layers transparently over Git, so GitHub, CI, and the merge queue are unaffected.

See `CLAUDE.md` at the repository root for the full jj workflow including branching, stacking, and the merge-of-all-work pattern.

## CI

CI runs on GitHub Actions.
The `build` job runs `bazel test //...` (including all lint targets) on every pull request.
The master branch is protected: the `build` job must pass and all merges go through the merge queue.

## Commit discipline

Each commit must do exactly one thing.
Commits must pass all linting and tests before being pushed.
Keep commits small: **400–500 diff lines maximum** (added + removed).
For larger changes, split into a sequence of focused commits.

Keep subject lines under 50 characters; put detail in the body.

## Contributing to the design docs

The design docs in `designdocs/` are the source of truth for architecture decisions.

1. Fork the repository and create a branch.
2. Make your changes to the relevant `.md` files in `designdocs/`.
3. Decisions that affect the architecture should be recorded in [Decisions.md](Decisions.md) as a new ADR, or as an update to an existing one.
4. Open a pull request with a clear description of what you changed and why.

For significant design changes, open an issue first to discuss the approach before writing a PR.

All `.md` files use **sentence-per-line** formatting: one sentence per line, blank lines between paragraphs.
This keeps diffs small and reviewable.

## Getting help

Open an issue on GitHub to ask questions or start a design discussion.
