use std::io::Write;

use anyhow::Context;
use clap::Subcommand;

use causes_proto::project_service_client::ProjectServiceClient;
use causes_proto::{
    CreateProjectRequest, DeleteProjectRequest, GetProjectRequest, ListProjectsRequest,
    RenameProjectRequest,
};

/// Project visibility for the CLI.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum CliVisibility {
    Public,
    Private,
}

impl CliVisibility {
    fn to_proto(&self) -> i32 {
        match self {
            Self::Public => causes_proto::project::Visibility::Public.into(),
            Self::Private => causes_proto::project::Visibility::Private.into(),
        }
    }
}

/// Project management commands.
#[derive(clap::Args, Debug)]
pub struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Subcommand, Debug)]
pub enum ProjectCommand {
    /// Create a new project.
    Create {
        /// Project name (slug: lowercase, alphanumeric, hyphens, 2-64 chars).
        name: String,
        /// Project description (Markdown).
        #[arg(long, default_value = "")]
        description: String,
        /// Visibility: public (default) or private.
        #[arg(long, default_value = "public")]
        visibility: CliVisibility,
    },
    /// Show a project.
    Get {
        /// Project name (slug).
        name: String,
    },
    /// List all projects.
    List,
    /// Rename a project.
    Rename {
        /// Current project name (slug).
        name: String,
        /// New name (slug).
        new_name: String,
    },
    /// Delete a project.
    Delete {
        /// Project name (slug).
        name: String,
    },
}

pub async fn run(
    server: &str,
    data_dir: &std::path::Path,
    args: ProjectArgs,
) -> anyhow::Result<()> {
    let mut out = std::io::stdout();
    match args.command {
        ProjectCommand::Create {
            name,
            description,
            visibility,
        } => create(server, data_dir, &name, &description, &visibility, &mut out).await,
        ProjectCommand::Get { name } => get(server, data_dir, &name, &mut out).await,
        ProjectCommand::List => list(server, data_dir, &mut out).await,
        ProjectCommand::Rename { name, new_name } => {
            rename(server, data_dir, &name, &new_name, &mut out).await
        }
        ProjectCommand::Delete { name } => delete(server, data_dir, &name, &mut out).await,
    }
}

fn format_project(out: &mut dyn Write, p: &causes_proto::Project) -> anyhow::Result<()> {
    writeln!(out, "Project ID:  {}", p.id)?;
    writeln!(out, "Name:        {}", p.name)?;
    if !p.description.is_empty() {
        writeln!(out, "Description: {}", p.description)?;
    }
    let vis = match causes_proto::project::Visibility::try_from(p.visibility) {
        Ok(causes_proto::project::Visibility::Public) => "public",
        Ok(causes_proto::project::Visibility::Private) => "private",
        _ => "unknown",
    };
    writeln!(out, "Visibility:  {vis}")?;
    if p.embargoed_by_default {
        writeln!(out, "Embargoed:   true")?;
    }
    Ok(())
}

async fn create(
    server: &str,
    data_dir: &std::path::Path,
    name: &str,
    description: &str,
    visibility: &CliVisibility,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let req = crate::rpc::authed_request(
        data_dir,
        server,
        CreateProjectRequest {
            name: name.to_owned(),
            description: description.to_owned(),
            visibility: visibility.to_proto(),
            embargoed_by_default: false,
        },
    )?;

    let mut client = ProjectServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    let resp = client
        .create_project(req)
        .await
        .context("CreateProject RPC failed")?
        .into_inner();

    if let Some(p) = resp.project {
        format_project(out, &p)?;
    }
    Ok(())
}

async fn get(
    server: &str,
    data_dir: &std::path::Path,
    name: &str,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let req = crate::rpc::authed_request(
        data_dir,
        server,
        GetProjectRequest {
            name: name.to_owned(),
        },
    )?;

    let mut client = ProjectServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    let resp = client
        .get_project(req)
        .await
        .context("GetProject RPC failed")?
        .into_inner();

    if let Some(p) = resp.project {
        format_project(out, &p)?;
    }
    Ok(())
}

async fn list(server: &str, data_dir: &std::path::Path, out: &mut dyn Write) -> anyhow::Result<()> {
    use tokio_stream::StreamExt;

    let req = crate::rpc::authed_request(data_dir, server, ListProjectsRequest {})?;

    let mut client = ProjectServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    let mut stream = client
        .list_projects(req)
        .await
        .context("ListProjects RPC failed")?
        .into_inner();

    let mut any = false;
    while let Some(batch) = stream.next().await {
        let batch = batch.context("ListProjects stream error")?;
        if !batch.projects.is_empty() {
            any = true;
            for p in &batch.projects {
                writeln!(out, "{}\t{}", p.id, p.name)?;
            }
        }
    }
    if !any {
        writeln!(out, "No projects.")?;
    }
    Ok(())
}

async fn rename(
    server: &str,
    data_dir: &std::path::Path,
    name: &str,
    new_name: &str,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let req = crate::rpc::authed_request(
        data_dir,
        server,
        RenameProjectRequest {
            name: name.to_owned(),
            new_name: new_name.to_owned(),
        },
    )?;

    let mut client = ProjectServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    let resp = client
        .rename_project(req)
        .await
        .context("RenameProject RPC failed")?
        .into_inner();

    if let Some(p) = resp.project {
        format_project(out, &p)?;
    }
    Ok(())
}

async fn delete(
    server: &str,
    data_dir: &std::path::Path,
    name: &str,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let req = crate::rpc::authed_request(
        data_dir,
        server,
        DeleteProjectRequest {
            name: name.to_owned(),
        },
    )?;

    let mut client = ProjectServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    client
        .delete_project(req)
        .await
        .context("DeleteProject RPC failed")?;

    writeln!(out, "Project {name} deleted.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::Parser;

    // ── Output formatting tests ──────────────────────────────────────

    fn sample_project() -> causes_proto::Project {
        causes_proto::Project {
            id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
            name: "my-project".to_owned(),
            description: "A test project".to_owned(),
            visibility: causes_proto::project::Visibility::Public.into(),
            embargoed_by_default: false,
            created_at: None,
        }
    }

    #[test]
    fn format_project_includes_all_fields() {
        let mut buf = Vec::new();
        format_project(&mut buf, &sample_project()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Project ID:  a1b2c3d4"));
        assert!(output.contains("Name:        my-project"));
        assert!(output.contains("Description: A test project"));
        assert!(output.contains("Visibility:  public"));
        assert!(!output.contains("Embargoed"));
    }

    #[test]
    fn format_project_omits_empty_description() {
        let mut buf = Vec::new();
        let mut p = sample_project();
        p.description = String::new();
        format_project(&mut buf, &p).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(!output.contains("Description"));
    }

    #[test]
    fn format_project_shows_embargoed() {
        let mut buf = Vec::new();
        let mut p = sample_project();
        p.embargoed_by_default = true;
        format_project(&mut buf, &p).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Embargoed:   true"));
    }

    #[test]
    fn format_project_shows_private_visibility() {
        let mut buf = Vec::new();
        let mut p = sample_project();
        p.visibility = causes_proto::project::Visibility::Private.into();
        format_project(&mut buf, &p).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Visibility:  private"));
    }

    // ── CLI parse tests ──────────────────────────────────────────────

    #[test]
    fn project_create_parses() {
        let cli = Cli::parse_from(["causes", "project", "create", "my-project"]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_create_private_parses() {
        let cli = Cli::parse_from([
            "causes",
            "project",
            "create",
            "my-project",
            "--visibility",
            "private",
        ]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_create_with_description_parses() {
        let cli = Cli::parse_from([
            "causes",
            "project",
            "create",
            "my-project",
            "--description",
            "hello",
        ]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_get_parses() {
        let cli = Cli::parse_from(["causes", "project", "get", "my-project"]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_list_parses() {
        let cli = Cli::parse_from(["causes", "project", "list"]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_rename_parses() {
        let cli = Cli::parse_from(["causes", "project", "rename", "my-project", "new-name"]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_delete_parses() {
        let cli = Cli::parse_from(["causes", "project", "delete", "my-project"]);
        assert!(matches!(cli.command, crate::Command::Project(_)));
    }

    #[test]
    fn project_requires_subcommand() {
        let result = Cli::try_parse_from(["causes", "project"]);
        assert!(result.is_err());
    }
}
