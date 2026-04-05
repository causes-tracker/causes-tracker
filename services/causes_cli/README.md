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
AI tools like Claude Code, Claude Desktop, VS Code, and Cursor can use this to interact with the Causes instance.

The MCP server requires `CAUSES_TOKEN` to be set.
Get a token with `causes auth login`, then copy it from `~/.local/share/causes/<server>.json`.

#### Claude Code configuration

Add to your Claude Code MCP settings:

```json
{
  "mcpServers": {
    "causes": {
      "command": "causes",
      "args": ["mcp"],
      "env": {
        "CAUSES_SERVER": "https://causes.example.com",
        "CAUSES_TOKEN": "<your-session-token>"
      }
    }
  }
}
```

#### Available tools

| Tool | Description |
|---|---|
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
| `CAUSES_TOKEN` | — | Session token (required for `causes mcp`) |
