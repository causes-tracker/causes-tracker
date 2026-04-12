# causes\_session

Session token persistence shared by the CLI and MCP server.

Both callers store tokens in the same data directory but with different filename suffixes to avoid collisions (`.json` for CLI, `_mcp.json` for MCP).
Token validation enforces the server's format (64 hex chars — two concatenated UUIDv4 simple representations).
