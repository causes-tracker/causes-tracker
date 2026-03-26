# Contributing to Causes

Thank you for your interest in contributing to Causes.
This document covers how to propose changes to the design docs and — when implementation begins — how to set up a development environment.

## Contributing to the design docs

The design docs in this directory are the primary artefact of the project right now.
All changes go through pull requests on GitHub.

1. Fork the repository and create a branch.
2. Make your changes to the relevant `.md` files.
3. Ensure your Markdown passes the linter before submitting (see [Linting](#linting) below).
4. Open a pull request with a clear description of what you changed and why.
5. Decisions that affect the architecture should be recorded in [Decisions.md](Decisions.md) as a new ADR, or as an update to an existing one.

For significant design changes, open an issue first to discuss the approach before writing a PR.

## Linting

Markdown files are linted with [markdownlint](https://github.com/DavidAnson/markdownlint).
The rules follow the default configuration.

To check locally (requires Node.js):

```sh
npm install -g markdownlint-cli
markdownlint designdocs/*.md
```

CI runs this automatically on every pull request.

## CI

> **Note:** CI configuration has not been set up yet.
> The following is the intended setup.

GitHub Actions will run the following jobs on every pull request:

| Job | What it checks |
| --- | --- |
| `lint-docs` | `markdownlint` on all `.md` files |
| `check-links` | [`markdown-link-check`](https://github.com/tcort/markdown-link-check) on all `.md` files |
| `lint` | Language linter (TBD pending stack decision) |
| `test` | Test suite (TBD pending stack decision) |
| `build` | Build and container image (TBD pending stack decision) |

All jobs must pass before a PR can be merged.

## Development environment (implementation — not yet started)

> **Note:** No implementation code exists yet.
> The language and stack are not decided (see [ADR-008](Decisions.md#adr-008-language-stack-open)).
> This section will be filled in once a stack is chosen.

Intended setup:

```sh
# Clone the repo
git clone https://github.com/<org>/causes.git
cd causes

# Start all services locally (Docker Compose)
docker compose up

# Run tests
<language-specific test command>

# Run linter
<language-specific lint command>
```

## Code style

Code style guidelines will be documented here once the language and stack are decided.
In the interim:

* Follow existing conventions in any file you edit.
* Run the linter before submitting a PR.
* Keep functions small and focused.
* Write tests for new behaviour.

## Commit messages

* Use the imperative mood in the subject line: "Add symptom deduplication" not "Added symptom deduplication".
* Keep the subject line under 72 characters.
* Reference relevant ADRs or issues in the body where applicable.

## Version control (jj)

We use [Jujutsu (`jj`)](https://github.com/jj-vcs/jj) for local version control.
See the workflow instructions in `CLAUDE.md` for day-to-day usage.

### One-time setup after cloning

The repository ships a jj repo-level config at `tools/jj-repo-config.toml`.
jj does not yet support versioning files inside `.jj/` directly
(see [jj managed config design](https://docs.jj-vcs.dev/latest/design/managed-config/) for future plans in this area),
so the file lives in `tools/` and must be symlinked into place once after cloning:

```sh
ln -s ../../tools/jj-repo-config.toml .jj/repo/config.toml
```

This config registers merge drivers for `Cargo.lock` and `MODULE.bazel.lock`
that regenerate those files automatically instead of leaving conflict markers
when `jj resolve --all` is run.

## Getting help

Open an issue on GitHub to ask questions or start a design discussion.
