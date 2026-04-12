use clap::Parser;

mod admin;
mod auth;
mod project;
pub(crate) mod rpc;
mod session_file;

/// Command-line interface for a Causes instance.
///
/// Authenticate and interact with a Causes API server.
/// The CLI never holds OAuth secrets — authentication is driven
/// entirely by the server via the device authorization flow.
#[derive(Parser, Debug)]
#[command(name = "causes", version, about)]
struct Cli {
    /// Causes API server address (e.g. http://localhost:50051).
    #[arg(long, env = "CAUSES_SERVER", default_value = "http://[::1]:50051")]
    server: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Instance administration (requires admin session).
    Admin(admin::AdminArgs),
    /// Manage authentication.
    Auth(auth::AuthArgs),
    /// Start MCP (Model Context Protocol) server on stdio.
    #[command(hide = true)]
    Mcp,
    /// Manage projects.
    Project(project::ProjectArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data_dir = session_file::default_data_dir();

    match cli.command {
        Command::Admin(args) => admin::run(&cli.server, &data_dir, args).await,
        Command::Auth(args) => auth::run(&cli.server, &data_dir, args).await,
        Command::Mcp => {
            use anyhow::Context;
            use rmcp::ServiceExt;
            let handler = causes_mcp::CausesTools::new(cli.server.clone(), data_dir.to_path_buf());
            let transport = rmcp::transport::io::stdio();
            let server = handler
                .serve(transport)
                .await
                .context("MCP server failed")?;
            server.waiting().await?;
            Ok(())
        }
        Command::Project(args) => project::run(&cli.server, &data_dir, args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_auth_subcommand() {
        // Just verify it parses without panicking; auth subcommands
        // will be tested in their own module.
        let result = Cli::try_parse_from(["causes", "auth"]);
        // "auth" alone should fail because it requires a sub-subcommand.
        assert!(result.is_err());
    }

    #[test]
    fn cli_help_contains_auth_and_default_server() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        cmd.write_long_help(&mut buf).expect("writing help");
        let help = String::from_utf8(buf).expect("help is valid UTF-8");
        assert!(help.contains("auth"), "help should mention auth subcommand");
        assert!(
            help.contains("[::1]:50051"),
            "help should show default server"
        );
    }

    #[test]
    fn cli_requires_subcommand() {
        let result = Cli::try_parse_from(["causes"]);
        assert!(result.is_err());
    }
}
