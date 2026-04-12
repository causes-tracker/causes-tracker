# api\_db

Database access layer for the Causes API.

Key decisions:

- **SQLx offline mode** (`SQLX_OFFLINE=true`): queries are checked against a cached `.sqlx/` directory at build time so that Bazel builds never need a running database.
  Run `bazel run //lib/rust/api_db:sqlx_prepare` to regenerate the cache after changing queries.
- **Hermetic test database**: tests spin up a throwaway PostgreSQL from a bundled tarball (`//infra/postgres`) rather than relying on a shared instance.
  This avoids cross-test contamination and means tests can run on any machine without setup.
- **Typed errors**: DB errors are mapped to domain-specific error enums, never stringified.
  See `ProjectError`, `AccountError`, etc.
