# proto\_ext

Shared gRPC extensions used by both the CLI and the API server.
Extracted into its own crate so that `causes_mcp` (which depends on tonic but not on the API server) can reuse the bearer-token interceptor without pulling in the full server dependency tree.
