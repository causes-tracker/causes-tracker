use clap::Subcommand;

/// Arguments for the `auth` subcommand group.
#[derive(clap::Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {}

pub fn run(_server: &str, args: AuthArgs) -> anyhow::Result<()> {
    match args.command {}
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn auth_requires_subcommand() {
        let result = Cli::try_parse_from(["causes", "auth"]);
        assert!(result.is_err());
    }
}
