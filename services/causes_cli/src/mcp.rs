use anyhow::Context;
use rmcp::ServiceExt;

/// Run the MCP server on stdio.
///
/// Reads `CAUSES_TOKEN` from the environment for gRPC authentication.
/// The server address comes from the `--server` CLI flag.
pub async fn run(server: &str) -> anyhow::Result<()> {
    let token = std::env::var("CAUSES_TOKEN")
        .context("CAUSES_TOKEN environment variable is required for MCP mode")?;

    let handler = causes_mcp::CausesTools::new(server.to_string(), token);

    let transport = rmcp::transport::io::stdio();
    let server = handler.serve(transport).await?;
    server.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_requires_causes_token() {
        // Verify that run() fails without CAUSES_TOKEN set.
        // We can't actually start the server (it would block on stdio),
        // but we can check the env var validation.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(run("http://unused:1")).unwrap_err();
        assert!(err.to_string().contains("CAUSES_TOKEN"), "got: {err}");
    }
}
