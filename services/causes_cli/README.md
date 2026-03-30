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

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `CAUSES_SERVER` | `http://[::1]:50051` | Causes API server address |
