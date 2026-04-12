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
