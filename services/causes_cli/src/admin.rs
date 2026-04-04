use anyhow::Context;
use clap::Subcommand;

use causes_proto::GrantRoleRequest;
use causes_proto::admin_service_client::AdminServiceClient;

/// Instance administration commands (requires an admin session).
#[derive(clap::Args, Debug)]
pub struct AdminArgs {
    #[command(subcommand)]
    command: AdminCommand,
}

/// Role values accepted by the CLI.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum CliRole {
    InstanceAdmin,
    Developer,
    ProjectMaintainer,
    SecurityTeam,
}

impl CliRole {
    fn to_proto(&self) -> causes_proto::Role {
        match self {
            Self::InstanceAdmin => causes_proto::Role::InstanceAdmin,
            Self::Developer => causes_proto::Role::Developer,
            Self::ProjectMaintainer => causes_proto::Role::ProjectMaintainer,
            Self::SecurityTeam => causes_proto::Role::SecurityTeam,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum AdminCommand {
    /// Grant a role to a user.
    GrantRole {
        /// Email address of the user to grant the role to.
        email: String,
        /// Role to grant.
        role: CliRole,
        /// Project name for project-scoped roles. Omit for instance-level.
        #[arg(long, default_value = "")]
        project: String,
    },
}

pub async fn run(server: &str, data_dir: &std::path::Path, args: AdminArgs) -> anyhow::Result<()> {
    match args.command {
        AdminCommand::GrantRole {
            email,
            role,
            project,
        } => grant_role(server, data_dir, &email, &role, &project).await,
    }
}

async fn grant_role(
    server: &str,
    data_dir: &std::path::Path,
    email: &str,
    role: &CliRole,
    project_id: &str,
) -> anyhow::Result<()> {
    let req = crate::rpc::authed_request(
        data_dir,
        server,
        GrantRoleRequest {
            email: email.to_owned(),
            role: role.to_proto().into(),
            project: project_id.to_owned(),
        },
    )?;

    let mut client = AdminServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    client
        .grant_role(req)
        .await
        .context("GrantRole RPC failed")?;

    println!("Role \"{role:?}\" granted to {email}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn admin_grant_role_parses() {
        let cli = Cli::parse_from([
            "causes",
            "admin",
            "grant-role",
            "friend@example.com",
            "developer",
        ]);
        assert!(matches!(cli.command, crate::Command::Admin(_)));
    }

    #[test]
    fn admin_grant_role_with_project_parses() {
        let cli = Cli::parse_from([
            "causes",
            "admin",
            "grant-role",
            "friend@example.com",
            "developer",
            "--project",
            "my-project",
        ]);
        assert!(matches!(cli.command, crate::Command::Admin(_)));
    }

    #[test]
    fn admin_requires_subcommand() {
        let result = Cli::try_parse_from(["causes", "admin"]);
        assert!(result.is_err());
    }
}
