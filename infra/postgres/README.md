# PostgreSQL infrastructure

This directory contains two PostgreSQL configurations:

- **Bazel test fixture** — an ephemeral server started inside `$TEST_TMPDIR` for unit tests.
  No Docker or host PostgreSQL required.
- **Docker Compose** — a persistent local dev server with pgvector.

## Local dev (Docker Compose)

`docker-compose.yml` lives at the repo root.

Start:

```sh
docker compose up -d
```

Connect:

```sh
psql postgresql://causes:causes@localhost:5432/causes
```

Stop (data preserved in the `causes-dev-pg` volume):

```sh
docker compose down
```

Full reset (deletes all data):

```sh
docker compose down -v
```

## Bazel test fixture

Tests that need a real PostgreSQL instance source
`infra/postgres/testfixture.sh` and call `pg_start`.
The fixture starts an ephemeral server on a free port and exports
`PGBIN`, `PGDATA`, `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, and
`TEST_POSTGRES_URL`.

See `testfixture_test.sh` for a working example.
