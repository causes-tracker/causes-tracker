# causes-api

Async Rust gRPC service that serves the Causes API.

## Bootstrap flow

On first start, if the `users` table is empty, the service initiates a
Google OAuth 2.0 Device Authorization Grant (RFC 8628):

1. Prints a one-time code to stdout:

   ```
   No administrators configured.
   Visit https://accounts.google.com/device and enter code: XXXX-YYYY
   ```

2. Polls Google's token endpoint until the user completes sign-in.
3. Verifies the returned `id_token` and creates the first `instance-admin`
   record in the database.
4. Prints `Admin <email> created. Instance is ready.` and continues serving
   gRPC.

No browser ever hits this server — the device flow is entirely server-initiated.

## Google OAuth app setup

You need a GCP project with an OAuth 2.0 client ID of type **TV and Limited
Input devices** before running the service for the first time.

1. Open the [Google Cloud Console](https://console.cloud.google.com/) and
   create or select a project.

2. Go to **APIs & Services → OAuth consent screen**.
   Configure the consent screen (app name, support email).
   Add the scopes `openid`, `email`, and `profile`.
   For internal use you can leave the app in **Testing** status and add your
   Google account as a test user.

3. Go to **APIs & Services → Credentials → Create Credentials → OAuth client
   ID** ([direct link](https://console.cloud.google.com/apis/credentials/oauthclient)).
   Choose application type **TV and Limited Input devices**.
   Note the **Client ID** and **Client Secret** — these become
   `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` below.

## Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | yes | — | PostgreSQL connection string, e.g. `postgresql://causes:causes@localhost:5432/causes` |
| `GOOGLE_CLIENT_ID` | bootstrap only | — | OAuth client ID from GCP Console (TV/Limited Input type) |
| `GOOGLE_CLIENT_SECRET` | bootstrap only | — | OAuth client secret paired with the above |
| `HONEYCOMB_API_KEY` | no | — | Honeycomb API key; when absent, traces are not exported |
| `HONEYCOMB_ENDPOINT` | no | `https://api.honeycomb.io:443` | OTLP endpoint; use `https://api.eu1.honeycomb.io:443` for EU |
| `BIND_ADDR` | no | `[::]:50051` | gRPC listen address (used when `TLS_DOMAIN` is unset) |
| `TLS_DOMAIN` | no | — | Domain for automatic TLS via Let's Encrypt (e.g. `causes.example.com`). When set, the server listens on port 443. |
| `TLS_ACME_EMAIL` | no | — | Contact email for Let's Encrypt certificate notifications |
| `TLS_CERT_CACHE_DIR` | no | `/var/lib/causes/certs` | Directory to cache TLS certificates; must persist across restarts |

`GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` are required for the bootstrap
flow and will also be required for Google social login once that is implemented.

Copy `env.example` to `.env` and fill in the values:

```sh
cp services/causes_api/env.example .env
$EDITOR .env
```

## Running locally

### Via Bazel (development)

```sh
# Start Postgres
docker compose up -d postgres

# Run the service (reads .env automatically)
bazel run //services/causes_api
```

### Via docker-compose (production-like)

```sh
# Build the OCI image and load it into Docker
bazel run //services/causes_api:image_load

# Start both Postgres and causes-api
docker compose up
```

The `image_load` step must be re-run after each code change.

Migrations run automatically on startup.
Run migrations manually:

```sh
DATABASE_URL=postgresql://causes:causes@localhost:5432/causes \
  bazel run //tools:sqlx -- migrate run \
  --source lib/rust/api_db/migrations
```

## TLS (production)

When `TLS_DOMAIN` is set, the server automatically obtains and renews a
Let's Encrypt certificate using the ACME TLS-ALPN-01 challenge.
gRPC and ACME challenges share port 443 via ALPN negotiation.

For local development, leave `TLS_DOMAIN` unset — the server runs plain
HTTP/2 on `BIND_ADDR` (default `[::]:50051`).

## Running tests

```sh
bazel test //services/causes_api:causes_api_test
```

## TLS

When `TLS_DOMAIN` is set, the server automatically obtains and renews a
Let's Encrypt certificate using the ACME TLS-ALPN-01 challenge.
gRPC and ACME challenges share port 443 via ALPN negotiation.
The first certificate issuance takes ~30 seconds after the server starts.
Subsequent renewals happen automatically ~30 days before expiry.

When `TLS_DOMAIN` is unset, the server runs plain HTTP/2 on `BIND_ADDR`
(default `[::]:50051`) with no TLS.

### Certificate cache

Certificates are cached in `TLS_CERT_CACHE_DIR` (default `/var/lib/causes/certs`).
This directory must persist across restarts — without it, a new certificate
would be issued on every start.
Let's Encrypt rate-limits to 5 certificates per domain per week.

For the causes-tracker AWS deployment, see [infra/terraform/README.md](../../infra/terraform/README.md#enabling-tls) for setup instructions.

## Troubleshooting bootstrap

**Bootstrap didn't trigger / no device code was printed:**

- Check that `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` are set.
  The service exits with an error if the `users` table is empty and these are
  unset.
- Check that the `users` table is actually empty:

  ```sh
  DATABASE_URL=postgresql://causes:causes@localhost:5432/causes \
    bazel run //infra/postgres:psql -- \
    -c "SELECT COUNT(*) FROM users;"
  ```

  If the count is non-zero, bootstrap has already run.
  To re-run it, delete the existing users rows (or drop and recreate the DB).

**"invalid_client" from Google:**

- Verify the client ID and secret match the OAuth client in GCP Console.
- Confirm the client type is **TV and Limited Input devices**, not Web or Desktop.

**"access_denied" from Google:**

- The Google account used to complete the device flow must be added as a test
  user in the OAuth consent screen if the app is still in **Testing** status.
