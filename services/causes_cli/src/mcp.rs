use anyhow::Context;
use rmcp::ServiceExt;

/// Run the MCP server on stdio.
///
/// The MCP server manages its own session via the `login` tool —
/// no pre-existing session is required.
pub async fn run(server: &str, data_dir: &std::path::Path) -> anyhow::Result<()> {
    let handler = causes_mcp::CausesTools::new(server.to_string(), data_dir.to_path_buf());

    let transport = rmcp::transport::io::stdio();
    let server = handler
        .serve(transport)
        .await
        .context("MCP server failed")?;
    server.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    /// The MCP handler is constructed correctly for a given server URL.
    /// Full tool behaviour is tested in the causes_mcp crate;
    /// this test just confirms the CLI glue layer wires up without panics.
    #[tokio::test]
    async fn handler_creates_without_panic() {
        let dir = std::env::temp_dir().join(format!("causes-mcp-cli-{}", std::process::id()));
        let _handler =
            causes_mcp::CausesTools::new("http://localhost:50051".to_string(), dir.clone());
        std::fs::remove_dir_all(&dir).ok();
    }
}
