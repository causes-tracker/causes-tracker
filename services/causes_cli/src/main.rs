use clap::Parser;

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
    /// Placeholder — subcommands will be added in follow-up PRs.
    #[command(hide = true)]
    Noop,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Noop => {
            println!("causes-cli connected to {}", cli.server);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_defaults() {
        let cli = Cli::parse_from(["causes", "noop"]);
        assert_eq!(cli.server, "http://[::1]:50051");
    }

    #[test]
    fn cli_accepts_server_override() {
        let cli = Cli::parse_from(["causes", "--server", "http://example.com:9090", "noop"]);
        assert_eq!(cli.server, "http://example.com:9090");
    }

    #[test]
    fn cli_help_does_not_panic() {
        // Verify the help text renders without panicking.
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        cmd.write_help(&mut buf).expect("writing help");
        let help = String::from_utf8(buf).expect("help is valid UTF-8");
        assert!(help.contains("Causes API server"));
    }

    #[test]
    fn cli_requires_subcommand() {
        let result = Cli::try_parse_from(["causes"]);
        assert!(result.is_err());
    }
}
