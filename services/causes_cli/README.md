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
On success, the session token is saved under `~/.local/share/causes/` (or `$XDG_DATA_HOME/causes/`).

### `auth whoami`

Show the currently authenticated user.
Reads the stored session token and calls the server's WhoAmI RPC.

## Session storage

Each server has its own session file, stored in `$XDG_DATA_HOME/causes/` (default `~/.local/share/causes/`).
The filename is derived from the server URL (e.g. `causes.example.com.json`).
Per the XDG Base Directory spec, credentials belong in `XDG_DATA_HOME`, not `XDG_CONFIG_HOME`.

### `mcp`

Start an MCP (Model Context Protocol) server on stdio.
AI tools like Claude Code, Claude Desktop, and VS Code can use this to interact with the Causes instance.

No pre-existing session is required.
On first use, invoke the `login` tool to authenticate via the device flow — the MCP server handles everything, including storing the session.

#### VS Code configuration

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "causes": {
      "type": "stdio",
      "command": "bazel",
      "args": ["--quiet", "run", "--", "//services/causes_cli", "--server=https://causes.example.com", "mcp"]
    }
  }
}
```

#### Available tools

| Tool | Description |
|---|---|
| `login` | Authenticate via device flow (one-time) |
| `whoami` | Show the authenticated user's identity |
| `list_projects` | List all visible projects |
| `get_project` | Get a project by name |
| `create_project` | Create a new project |
| `rename_project` | Rename a project |
| `delete_project` | Delete a project |

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `CAUSES_SERVER` | `http://[::1]:50051` | Causes API server address |
