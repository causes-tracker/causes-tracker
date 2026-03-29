# causes-cli

Command-line interface for interacting with a Causes API server.

The CLI communicates exclusively over gRPC.
It never holds OAuth secrets — authentication is driven entirely by the server via the device authorization flow.

## Building

```sh
bazel build //services/causes_cli
```

## Usage

```sh
bazel run //services/causes_cli -- --help
```

The `--server` flag (env: `CAUSES_SERVER`) sets the API server address.
It defaults to `http://[::1]:50051` for local development.

Authentication commands are grouped under the `auth` subcommand:

```sh
bazel run //services/causes_cli -- auth --help
```

## Commands

### `auth login`

Log in to a Causes instance via the device authorization flow.
The server drives the entire OAuth flow — the CLI just displays a code and polls for completion.
On success, the session token is saved to `~/.local/share/causes/session.json` (or `$XDG_DATA_HOME/causes/session.json`).

## Session storage

The session token is stored in `$XDG_DATA_HOME/causes/session.json` (default `~/.local/share/causes/session.json`).
The file contains the token and the server address it was issued for.
Per the XDG Base Directory spec, credentials belong in `XDG_DATA_HOME`, not `XDG_CONFIG_HOME`.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `CAUSES_SERVER` | `http://[::1]:50051` | Causes API server address |
