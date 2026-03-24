# instance-api — operator setup guide

The instance API is the core gRPC server for a Causes instance.
On first start it bootstraps the initial administrator account via Google's
OAuth 2.0 Device Authorization Grant (RFC 8628).
No browser needs to reach this server — all OAuth interaction happens at
Google's own website.

## Prerequisites

### 1. Register a Google OAuth 2.0 client

Google's device authorization grant requires an OAuth 2.0 client registered
in Google Cloud Console.
OIDC is an open standard, but Google only issues tokens to registered clients;
this one-time step is required for any deployment that uses Google as the
identity provider.

1. Open https://console.cloud.google.com/apis/credentials
2. Create a project (or select an existing one)
3. Click **Enable APIs and Services** → search for **Google+ API** or
   **People API** → enable it
4. Click **Create credentials** → **OAuth 2.0 Client ID**
5. Application type: **TV and Limited Input devices**
6. Give it a name (e.g. `causes-instance`) → **Create**
7. Copy the **Client ID** and **Client Secret** — you will need both below

This client registration can be reused across restarts and deployments.
It does not need a redirect URI because device flow never redirects to your
server.

### 2. Create a `.env` file

Copy `.env.example` to `.env` and fill in the values:

```sh
cp .env.example .env
$EDITOR .env
```

`.env` is gitignored and must not be committed.

## Environment variables

| Variable | Required | Description |
|---|---|---|
| `DATABASE_URL` | Yes | PostgreSQL connection string, e.g. `postgresql://causes:causes@postgres:5432/causes` |
| `GOOGLE_CLIENT_ID` | Yes | Client ID from the Google Cloud Console step above |
| `GOOGLE_CLIENT_SECRET` | Yes | Client Secret from the same registration |
| `HONEYCOMB_API_KEY` | No | If set, traces are exported to Honeycomb via OTLP. Get a key at https://ui.honeycomb.io/account |
| `BIND_ADDR` | No | gRPC listen address (default: `[::]:50051`) |

### Honeycomb tracing

When `HONEYCOMB_API_KEY` is set the server exports OpenTelemetry traces to
`https://api.honeycomb.io:443` using the `x-honeycomb-team` header.
When unset, structured JSON logs are written to stdout only.

## First-time setup flow

On first start with an empty database the server will:

1. Call Google's device authorization endpoint with the configured client ID
2. Print a prompt to stdout:
   ```
   No administrators configured.
   Visit https://accounts.google.com/device
   and enter code: XXXX-YYYY
   ```
3. Poll Google's token endpoint in the background
4. Once you complete the login at Google, the server creates the admin user
   and prints:
   ```
   Admin <email> created. Instance is ready.
   ```
5. The gRPC server continues running normally

No HTTP endpoint on this server is involved in this flow.

## Running locally with docker-compose

```sh
# Build and load the image (amd64)
bazel build //services/instance-api:image_tarball
docker load < bazel-bin/services/instance-api/image_tarball/tarball.tar

# Start postgres + instance-api
docker-compose up

# Watch for the device code prompt in the logs
docker-compose logs -f instance-api
```

## Running on AWS (EC2 t4g.nano + Cloudflare Tunnel)

```sh
# Build for ARM64
bazel build //services/instance-api:image_tarball \
  --platforms=@rules_rs//rust/platform:linux_arm64

# Deploy with OpenTofu (creates EC2, EIP, Cloudflare Tunnel)
cd infra/terraform
bazel run //infra:tofu -- apply
```

After first boot, SSH to the EC2 instance and watch the logs:

```sh
docker logs instance-api -f
```

## Troubleshooting

### Check registered administrators (without DB access)

```sh
grpcurl -plaintext localhost:50051 causes.v1.AdminService/ListUsers
```

### Verify external identities for a user

```sh
grpcurl -plaintext -d '{"user_id": "<id>"}' \
  localhost:50051 causes.v1.AdminService/GetUserIdentities
```

### Re-run bootstrap (if no admins exist after a restart)

The bootstrap runs automatically on startup whenever the `users` table is
empty.
Delete the admin row and restart the container:

```sh
psql "$DATABASE_URL" -c "DELETE FROM role_assignments; DELETE FROM external_identities; DELETE FROM users;"
docker-compose restart instance-api
```
