# causes-api

Async Rust gRPC service that serves the Causes API.

## Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | yes | — | PostgreSQL connection string, e.g. `postgresql://causes:causes@localhost:5432/causes` |
| `HONEYCOMB_API_KEY` | no | — | Honeycomb API key; when absent, traces are not exported |
| `HONEYCOMB_ENDPOINT` | no | `https://api.honeycomb.io:443` | OTLP endpoint; use `https://api.eu1.honeycomb.io:443` for EU |
| `BIND_ADDR` | no | `[::]:50051` | gRPC listen address |

Copy `env.example` to `.env` and fill in the values:

```sh
cp env.example .env
$EDITOR .env
```

## Running locally

```sh
# Start Postgres
docker compose up -d postgres

# Run migrations and start the server
bazel run //services/causes_api
```

Migrations run automatically on startup.
To run them manually (e.g. to inspect schema state before starting the server):

```sh
DATABASE_URL=postgresql://causes:causes@localhost:5432/causes \
  bazel run //tools:sqlx -- migrate run \
  --source services/causes_api/migrations
```

## Running tests

```sh
bazel test //services/causes_api:causes_api_test
```
