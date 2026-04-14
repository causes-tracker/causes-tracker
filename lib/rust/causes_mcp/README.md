# causes_mcp

MCP (Model Context Protocol) server library for Causes.
Exposes bug tracker operations as MCP tools that AI assistants can invoke.

This crate is transport-agnostic — it provides the tool definitions and gRPC client logic.
Callers bind it to a transport:
- **stdio**: `causes mcp` CLI subcommand (Claude Code, IDE integrations)
- **Streamable HTTP**: BFF endpoint (Claude on the web)

## Authentication

The MCP server manages its own session via a `login` tool.
On first use, Claude invokes `login` which drives the device authorization flow:
the tool returns a verification URL and code, the user completes sign-in in their browser,
and the tool long-polls until done.
The resulting session token is stored in an MCP-specific file (`<server>_mcp.json`)
separate from the CLI's session to avoid interference.

## rmcp version

Uses rmcp v1.x.
The macro API changed significantly between 0.x and 1.x:
- `#[tool_router]` on the impl block containing tool methods
- `#[tool_handler]` on the `ServerHandler` impl
- `ToolRouter<Self>` field on the struct, initialized with `Self::tool_router()`
- `Parameters<T>` wrapper for tool inputs (T must derive `Deserialize` + `JsonSchema`)
