//! Project management tools exposed via MCP.

use causes_proto::project_service_client::ProjectServiceClient;
use causes_proto::{
    CreateProjectRequest, DeleteProjectRequest, GetProjectRequest, ListProjectsRequest,
    RenameProjectRequest,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::CausesTools;

fn format_project(p: &causes_proto::Project) -> String {
    let vis = match p.visibility {
        1 => "public",
        2 => "private",
        _ => "unknown",
    };
    format!(
        "Name: {}\nDescription: {}\nVisibility: {}\nID: {}",
        p.name, p.description, vis, p.id,
    )
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ProjectName {
    /// Project name (slug).
    pub(crate) name: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct CreateProjectParams {
    /// Project name (slug, lowercase alphanumeric + hyphens).
    pub(crate) name: String,
    /// Project description.
    pub(crate) description: String,
    /// Visibility: "public" or "private". Defaults to "public".
    #[serde(default = "default_public")]
    pub(crate) visibility: String,
}

fn default_public() -> String {
    "public".to_string()
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct RenameProjectParams {
    /// Current project name.
    pub(crate) name: String,
    /// New project name.
    pub(crate) new_name: String,
}

#[tool_router(router = project_router, vis = "pub(crate)")]
impl CausesTools {
    /// List all projects visible to the authenticated user.
    #[tool(description = "List all projects visible to the authenticated user")]
    pub(crate) async fn list_projects(&self) -> String {
        let mut client = match self.authed_channel().await {
            Ok(c) => ProjectServiceClient::new(c),
            Err(e) => return e,
        };

        match client.list_projects(ListProjectsRequest {}).await {
            Ok(resp) => {
                let mut stream = resp.into_inner();
                let mut projects = Vec::new();
                while let Some(batch) = tokio_stream::StreamExt::next(&mut stream).await {
                    match batch {
                        Ok(b) => projects.extend(b.projects),
                        Err(e) => return format!("Stream error: {}", e.message()),
                    }
                }
                if projects.is_empty() {
                    return "No projects found.".to_string();
                }
                projects
                    .iter()
                    .map(format_project)
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            }
            Err(e) => format!("ListProjects failed: {}", e.message()),
        }
    }

    /// Get a project by name.
    #[tool(description = "Get a project by its name (slug)")]
    pub(crate) async fn get_project(&self, params: Parameters<ProjectName>) -> String {
        let mut client = match self.authed_channel().await {
            Ok(c) => ProjectServiceClient::new(c),
            Err(e) => return e,
        };

        match client
            .get_project(GetProjectRequest {
                name: params.0.name,
            })
            .await
        {
            Ok(resp) => match resp.into_inner().project {
                Some(p) => format_project(&p),
                None => "Project not found.".to_string(),
            },
            Err(e) => format!("GetProject failed: {}", e.message()),
        }
    }

    /// Create a new project.
    #[tool(description = "Create a new project. Requires the developer role.")]
    pub(crate) async fn create_project(&self, params: Parameters<CreateProjectParams>) -> String {
        let mut client = match self.authed_channel().await {
            Ok(c) => ProjectServiceClient::new(c),
            Err(e) => return e,
        };

        let visibility = match params.0.visibility.as_str() {
            "private" => 2,
            _ => 1,
        };

        match client
            .create_project(CreateProjectRequest {
                name: params.0.name,
                description: params.0.description,
                visibility,
                embargoed_by_default: false,
            })
            .await
        {
            Ok(resp) => match resp.into_inner().project {
                Some(p) => format!("Created project:\n{}", format_project(&p)),
                None => "Project created but no details returned.".to_string(),
            },
            Err(e) => format!("CreateProject failed: {}", e.message()),
        }
    }

    /// Rename a project.
    #[tool(description = "Rename a project. Requires project-maintainer or instance-admin.")]
    pub(crate) async fn rename_project(&self, params: Parameters<RenameProjectParams>) -> String {
        let mut client = match self.authed_channel().await {
            Ok(c) => ProjectServiceClient::new(c),
            Err(e) => return e,
        };

        match client
            .rename_project(RenameProjectRequest {
                name: params.0.name,
                new_name: params.0.new_name,
            })
            .await
        {
            Ok(resp) => match resp.into_inner().project {
                Some(p) => format!("Renamed project:\n{}", format_project(&p)),
                None => "Project renamed but no details returned.".to_string(),
            },
            Err(e) => format!("RenameProject failed: {}", e.message()),
        }
    }

    /// Delete a project.
    #[tool(
        description = "Delete a project and all associated data. Requires project-maintainer or instance-admin."
    )]
    pub(crate) async fn delete_project(&self, params: Parameters<ProjectName>) -> String {
        let mut client = match self.authed_channel().await {
            Ok(c) => ProjectServiceClient::new(c),
            Err(e) => return e,
        };

        match client
            .delete_project(DeleteProjectRequest {
                name: params.0.name,
            })
            .await
        {
            Ok(_) => "Project deleted.".to_string(),
            Err(e) => format!("DeleteProject failed: {}", e.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::CausesTools;

    use causes_proto::project_service_server::{ProjectService, ProjectServiceServer};
    use causes_proto::*;

    struct MockProjectService;

    impl MockProjectService {
        fn test_project() -> Project {
            Project {
                id: "proj-1".to_string(),
                name: "test-project".to_string(),
                description: "A test project".to_string(),
                visibility: 1,
                embargoed_by_default: false,
                created_at: None,
            }
        }
    }

    #[tonic::async_trait]
    impl ProjectService for MockProjectService {
        async fn create_project(
            &self,
            _req: tonic::Request<CreateProjectRequest>,
        ) -> Result<tonic::Response<CreateProjectResponse>, tonic::Status> {
            Ok(tonic::Response::new(CreateProjectResponse {
                project: Some(Self::test_project()),
            }))
        }

        async fn get_project(
            &self,
            _req: tonic::Request<GetProjectRequest>,
        ) -> Result<tonic::Response<GetProjectResponse>, tonic::Status> {
            Ok(tonic::Response::new(GetProjectResponse {
                project: Some(Self::test_project()),
            }))
        }

        type ListProjectsStream = tokio_stream::Once<Result<ListProjectsResponse, tonic::Status>>;

        async fn list_projects(
            &self,
            _req: tonic::Request<ListProjectsRequest>,
        ) -> Result<tonic::Response<Self::ListProjectsStream>, tonic::Status> {
            Ok(tonic::Response::new(tokio_stream::once(Ok(
                ListProjectsResponse {
                    projects: vec![Self::test_project()],
                },
            ))))
        }

        async fn rename_project(
            &self,
            _req: tonic::Request<RenameProjectRequest>,
        ) -> Result<tonic::Response<RenameProjectResponse>, tonic::Status> {
            Ok(tonic::Response::new(RenameProjectResponse {
                project: Some(Self::test_project()),
            }))
        }

        async fn delete_project(
            &self,
            _req: tonic::Request<DeleteProjectRequest>,
        ) -> Result<tonic::Response<DeleteProjectResponse>, tonic::Status> {
            Ok(tonic::Response::new(DeleteProjectResponse {}))
        }
    }

    /// Start a mock project gRPC server and return an authenticated CausesTools.
    async fn authed_tools() -> CausesTools {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ProjectServiceServer::new(MockProjectService))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let channel = tonic::transport::Channel::from_shared(url.clone())
            .unwrap()
            .connect_lazy();
        let tools =
            CausesTools::with_channel(url, Arc::new(causes_session::NullSessionStore), channel);
        tools.set_authed_channel(&"d".repeat(64)).await;
        tools
    }

    #[tokio::test]
    async fn list_projects_returns_projects() {
        let tools = authed_tools().await;
        let result = tools.list_projects().await;
        assert!(result.contains("test-project"), "got: {result}");
        assert!(result.contains("public"), "got: {result}");
    }

    #[tokio::test]
    async fn get_project_returns_details() {
        let tools = authed_tools().await;
        let result = tools
            .get_project(Parameters(ProjectName {
                name: "test-project".to_string(),
            }))
            .await;
        assert!(result.contains("test-project"), "got: {result}");
        assert!(result.contains("A test project"), "got: {result}");
    }

    #[tokio::test]
    async fn create_project_returns_created() {
        let tools = authed_tools().await;
        let result = tools
            .create_project(Parameters(CreateProjectParams {
                name: "new-project".to_string(),
                description: "desc".to_string(),
                visibility: "public".to_string(),
            }))
            .await;
        assert!(result.contains("Created project"), "got: {result}");
    }

    #[tokio::test]
    async fn rename_project_returns_renamed() {
        let tools = authed_tools().await;
        let result = tools
            .rename_project(Parameters(RenameProjectParams {
                name: "old".to_string(),
                new_name: "new".to_string(),
            }))
            .await;
        assert!(result.contains("Renamed project"), "got: {result}");
    }

    #[tokio::test]
    async fn delete_project_returns_deleted() {
        let tools = authed_tools().await;
        let result = tools
            .delete_project(Parameters(ProjectName {
                name: "test-project".to_string(),
            }))
            .await;
        assert_eq!(result, "Project deleted.");
    }
}
