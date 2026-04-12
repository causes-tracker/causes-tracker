# infra

Infrastructure tooling.
All CLI tools (AWS, OpenTofu) are hermetically managed through Bazel — no host installs required.

Subdirectories:

- `github/` — GitHub org settings managed via OpenTofu
- `postgres/` — hermetic PostgreSQL for local dev and tests (no system Postgres needed)
- `terraform/` — cloud infrastructure managed via OpenTofu

OpenTofu modules are run via `bazel run //infra:tofu -- <module> <command>` rather than a bare `tofu` CLI so that the version is pinned and reproducible.
