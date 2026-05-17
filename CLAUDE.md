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

The same convention applies to Rust doc comments (`///`, `//!`, prose `//`) and to PR and issue descriptions, for the same rebase-conflict-reduction reason.

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

`//infra:tofu` auto-`cd`s into the requested module (e.g. `bazel run //infra:tofu -- infra/terraform plan`), so module-relative args go after the module path.

Native tooling must not appear in *documented* commands either — no `cargo build` / `psql` / `tofu` in READMEs, design docs, or inline comments. Use the Bazel wrapper or omit the example.

When a `bazel` invocation is meant to be consumed for its stdout — captured into a variable, shown in a doc, used as an example — put `--quiet` between `bazel` and the subcommand (e.g. `bazel --quiet run //infra:tofu -- output -raw images_bucket`).
The default streaming progress output otherwise buries the actual command output.
Not needed for top-level `bazel build //...` / `bazel test //...` where progress is the point.

## Incremental development

When building a feature incrementally across multiple commits, do not expose new **user-facing** interfaces until the feature is ready.
CLI flags, environment variables, `--help` text, API endpoints, and documentation should not be visible to users until the implementing code lands in the same or a prior commit.
Internal code (traits, modules, functions, DB schema) can land before its callers — that is normal incremental development.
Use `#[cfg]` attributes, feature flags, or simply defer adding the user-facing interface to the commit that adds the implementation.

The same rule extends past the user-facing surface: each commit must stand alone if no later commit ever lands.
That means no README claims, config-file sections, dead-code allow-attributes, or struct fields that only make sense once a later commit lands.
A `[regression.trend]` config section, an unreachable `RunMetrics.bazel: BazelStats` field, or a README bullet describing a future subcommand are all forward references.
If a downstream commit is later dropped, anything that promised its arrival becomes a lie in the merged tree.

## Testing

Tests must never mutate global process state.
In particular, never call `std::env::set_var` in tests — it is unsound in multithreaded programs (Rust 2024 marks it `unsafe`) and causes flaky failures when tests run in parallel.
If code reads environment variables, refactor it to accept the value as a parameter so tests can pass it directly.

Every feature or fix lands test-first: write a test, run it and confirm it fails for the right reason, implement the change, then confirm it passes.
This applies to every kind of test — `bazel test` targets, shell test assertions, integration checks, and manual `bazel run` verification steps.
Skipping straight to implementation means neither the test nor the implementation has been verified to do its job.

If a piece of code is hard to test, the answer is to make it testable, not to skip the test.
External services are mockable (wiremock, in-process servers, custom URLs).
"Needs a real ACME server" / "needs a live Slack webhook" is not an exemption — every external surface can be stood up locally or stubbed.
If a function is too entangled to test, refactor it until it is, then test it.

## Type discipline

Distinct domain concepts get distinct types — newtypes everywhere, including trait signatures, function parameters, and struct fields.
A trait method that takes `&str` for what is semantically an `Email` should take `&Email` instead; same for `DisplayName`, `AuthProvider`, `Subject`, etc.
Newtypes validate at construction; without them the compiler can't catch argument-order mistakes between values that share a primitive shape.

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

Toolchain ergonomics ship with the toolchain.
When a commit adds a build-system feature (e.g. `rules_rs`), it also adds the wrapper aliases (`//tools:cargo`, `//tools:rustc`, `//tools:rustfmt`) and the minimum scaffolding (an empty `Cargo.toml` workspace) that proves the toolchain works.
The first real crate and its dependencies go in the commit that introduces the consuming service, not the toolchain commit.

The project's quality gates (lint, format, test, coverage, per-file thresholds) run automatically via the pre-push and end-of-turn hooks.
If a gate fails, fix the underlying issue.
Never introduce a suppression marker (`# shellcheck disable`, `#[allow(...)]`, `// eslint-disable`, `// @ts-ignore`, `# noqa`, `# type: ignore`, `#[ignore]`) or edit a gate-config file (`.bazelignore`, `.clippy.toml`, `rustfmt.toml`, `.shellcheckrc`, `.yamlfmt`, `tools/check.sh`) — the end-of-turn hook will block any such change.
Suppression also covers the softer forms — weakening an assertion, inverting a check to match wrong output, swallowing an error with `.ok()` or a discard binding, returning early from a test, narrowing the platforms a test runs on, or scoping a target out of the default build.
If a fix path looks like one of those, stop and ask before applying it.

Prefer fixing problems in the Rust application layer over working around them in infrastructure.
A scheduled job that restarts a service to mask stale DB credentials is the wrong shape; the credential lifecycle belongs in the application code.

For `bazel run` targets (servers, binaries): run them and confirm they start correctly before pushing — even long-running ones, which should be started, verified, then killed.
If the environment can't run the target, say so explicitly rather than pushing untested.

There are three distinct levels of confidence, and they are not interchangeable:

1. **It builds.** Compilation succeeded. Says nothing about runtime behavior.
2. **It runs / exits 0.** The process started, ran to completion, and returned a success status. Strictly stronger than (1) — the binary actually executed — but still says nothing about whether the work it claims to have done was actually done. A site generator can exit 0 and produce zero pages. A test runner can exit 0 with no assertions. A server can come up and then 404 every route.
3. **It works.** The observable outcome the change is supposed to produce is actually present. This is the only level that justifies declaring a task done.

Always reach level (3) before claiming completion. Concrete moves:

- HTTP servers: `curl -s -o /dev/null -w "%{http_code}" <url>` against the routes that matter; a non-2xx is a fail even if the server is "running".
- Build outputs: list the produced files, check their sizes, grep for content that should be there. A 200-byte index.html where you expected 200kB is a fail.
- Tests: assert on the real invariant (pages exist, fields populated, errors classified) — not just `result.is_ok()` and not just that the test binary exited cleanly.

The distinction matters because (2) feels like evidence of correctness and isn't.
Most "passed locally but broken in review" incidents in this project were level-(2) claims dressed up as level-(3) ones.

## Strategic decisions

When working autonomously, scope growth alone is not a reason to stop and ask — bigger surface inside the same kind of choice is fine to handle straight through ("two Cargo lines" growing to "two Cargo lines + 10 lines of plumbing in the same crate" is the same decision, just larger).
Pause and check in when the *axis* of the decision shifts: programming language, dependency model (pure-Rust → bundled C library), process model (in-process → service/sidecar), linking model (static → dynamic), license category (permissive → copyleft), sync↔async where the change reshapes callers, in-tree fix → upstream patch/fork, or adding a new runtime (JVM, Python interpreter) to a service.

The operational test before proceeding: would a reasonable developer question this path if they'd given you the original mission and saw the route you're now taking?
If yes, escalate with a one-paragraph "here's where this is going, here are the options, which axis do you want me to optimise."
If no, proceed.
"Don't pause for clarifying questions" means don't pause on logistics or trivial preference; it does not mean silently widen the strategic surface.

## Code comments

Comments document what the code is, not the infinite space of options ruled out.
Strip phrases like "we do NOT…", "is omitted deliberately because…", "out of scope — would need…", "encodes the subset of…".
"Why we picked this shape over that one" belongs in the PR description; the code is the wrong place for it.
Keep one-line statements of behaviour, and add a *why* only when it would not be obvious from the code itself — a hidden constraint, a subtle invariant, or a workaround for a specific bug.

## Communication style

Do not validate or assess the user's points ("You're right", "Good question", "Great point", "honest answer", "fair point", etc.).
When the user makes a correction or observation, respond with substance — the edit, the counterpoint, the next step.

Do not perform accountability when acknowledging an error.
Phrases like "owning that", "I'll do better", "I learned my lesson", "I'll remember this", or attributing the cause to a narrower sub-phase ("introduced in the tack-on", "from a later edit") are hollow: across sessions there is no continuous self that can be accountable, and within a session, narrating accountability is decoration not learning.
State the substance: what is wrong, what is correct, what changes — no commitment to a future improvement, no retro-blame on a sub-event of the same authorship.

When drafting issues, PRs, comments, design docs, or any other written prose: be terse.
One sentence per point, no warm-up.
Don't restate the title in the body.
Don't narrate what a code snippet does — the snippet carries its own meaning.
Cut hedging adjectives ("modern", "silent", "tedious") and parentheticals about well-known facts.
Skip "Workaround today" / "Why this matters" sections unless the workaround or rationale is genuinely non-obvious.
Aim for half the length that feels natural, then cut again.

## PR review workflow

When fetching review comments to address, use the GraphQL `reviewThreads` query — not the REST `/repos/.../pulls/{n}/comments` endpoint.
The REST listing hides reviewer follow-ups added to existing threads; GraphQL returns every thread with all its comments in order plus the `isResolved` state.

```sh
gh api graphql -f query='
{
  repository(owner: "OWNER", name: "REPO") {
    pullRequest(number: N) {
      reviewThreads(first: 50) {
        nodes {
          id isResolved path line
          comments(first: 20) {
            nodes { databaseId author { login } body createdAt }
          }
        }
      }
    }
  }
}'
```

Walk every thread; address every comment whose author is the reviewer and that has no later `(claude):`-prefixed reply from you.
Follow-ups inside resolved-looking threads count the same as top-level ones.

After squashing the fix that addresses a comment, post a reply describing the change — don't just push silently.
Reply via REST keyed on `databaseId` from the GraphQL response:

```sh
gh api repos/OWNER/REPO/pulls/N/comments/<databaseId>/replies -X POST -f body='(claude): …'
```

Every Claude-authored reply body **must** start with `(claude):` followed by a space, so reviewers can distinguish your replies from the PR author's own.
Missing replies on addressed comments are a regression even if context was compacted across sessions; check before declaring work done.

Never `DELETE /reviews/<id>` on a PR review whose state is `PENDING`.
PENDING means a reviewer started a review with draft comments not yet submitted, and the DELETE wipes the review *and every draft comment inside it* with no recovery.
If `POST .../replies` returns `422 user_id can only have one pending review per pull request`, find out who owns the pending review first:

```sh
gh api repos/OWNER/REPO/pulls/N/reviews --jq '.[] | select(.state == "PENDING") | .user.login'
```

If it's yours, submit or delete it.
If it's someone else's, leave it alone — reply via an issue-level comment, wait for them to submit, or ask them directly.

## External API clients

When integrating with an external HTTP/RPC service (GitHub, BuildBuddy, Slack, anything), mock-only test suites prove your code matches *your model* of the API, not that your model matches the API.
The failures mocks miss are the failures that matter: field naming case, response shape variations by state, semantically-overloaded join keys, undocumented filter behaviour, `200 OK` + `{}` returned when a query field is misspelled (many APIs silently treat unknown fields as "no filter" rather than rejecting them).

Before declaring an integration complete:

- Make at least one real call against the live service.
- Diff the real response against the mock's expected shape; differences mean the tests were hallucinations — update both the code and the mocks from the real response.
- Sanity-check filters by querying for something you know exists; a `{}` response is the same shape as "no results" and "unknown field name".
- Source credentials from a file or env at the boundary (e.g. `awk -F= '/key=/{print $NF}' .bazelrc.user` into a shell variable). Never inline them on the command line — they end up in shell history and session transcripts.

BuildBuddy's `SearchInvocation` is the canonical reminder: request fields are snake_case (`commit_sha`, `repo_url`, `branch_name`); response fields are camelCase (`commitSha`, `cacheStats.actionCacheHits`); camelCase requests return `{}` silently; for `pull_request` GitHub events the `commit_sha` is the Actions-synthesised merge SHA, not the PR head SHA.

## Environment

Development happens in disposable containers.
Only save session-local memories (ephemeral, current-conversation-only) outside the repo.
Durable memories (feedback, preferences, project context) must go in-repo (CLAUDE.md, skill files, etc.) so they survive container destruction.

## Git workflow

This project uses [Jujutsu (`jj`)](https://github.com/jj-vcs/jj) for local version control.
See `.claude/skills/jj/` for complete jj workflow guidance.

## Claude rules

Per-project Claude feedback, gotchas, and project history live under `.claude/rules/`.
Every `.md` file in that directory tree auto-loads at the start of each Claude Code session, so the constraints they describe are always in effect — no index required.
Entries with `paths:` frontmatter (a list of globs) load only when Claude reads matching files, keeping context lean for narrowly-scoped rules.

When the user gives feedback worth keeping across sessions, add a new file under `.claude/rules/feedback/` rather than relying on the in-container memory dir (which is destroyed when the container is rebuilt).
Project status notes (in-flight initiatives, decided architectural directions) go under `.claude/rules/project/`.
