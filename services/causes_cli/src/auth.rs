use clap::Subcommand;

/// Arguments for the `auth` subcommand group.
#[derive(clap::Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Log in to a Causes instance via device authorization flow.
    Login,
    /// Show the currently authenticated user.
    #[command(name = "whoami")]
    WhoAmI,
}

pub fn run(_server: &str, args: AuthArgs) -> anyhow::Result<()> {
    match args.command {
        AuthCommand::Login => {
            anyhow::bail!("auth login not yet implemented");
        }
        AuthCommand::WhoAmI => {
            anyhow::bail!("auth whoami not yet implemented");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn auth_login_parses() {
        let cli = Cli::parse_from(["causes", "auth", "login"]);
        assert!(matches!(cli.command, crate::Command::Auth(_)));
    }

    #[test]
    fn auth_whoami_parses() {
        let cli = Cli::parse_from(["causes", "auth", "whoami"]);
        assert!(matches!(cli.command, crate::Command::Auth(_)));
    }

    #[test]
    fn auth_requires_subcommand() {
        let result = Cli::try_parse_from(["causes", "auth"]);
        assert!(result.is_err());
    }
}
