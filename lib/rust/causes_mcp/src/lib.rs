use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;

use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::Deserialize;

use causes_proto::WhoAmIRequest;
use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::project_service_client::ProjectServiceClient;
use causes_proto::{
    CreateProjectRequest, DeleteProjectRequest, GetProjectRequest, ListProjectsRequest,
    RenameProjectRequest,
};
use tonic::transport::Channel;

/// MCP server exposing Causes bug tracker tools.
///
/// Transport-agnostic: callers provide stdio or HTTP transport.
/// The server connects to a Causes gRPC API using a Bearer token.
#[derive(Clone)]
pub struct CausesTools {
    server_url: String,
    token: String,
    tool_router: ToolRouter<Self>,
}

impl CausesTools {
    pub fn new(server_url: String, token: String) -> Self {
        Self {
            server_url,
            token,
            tool_router: Self::tool_router(),
        }
    }

    async fn authed_channel(
        &self,
    ) -> Result<tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>, String>
    {
        let channel = Channel::from_shared(self.server_url.clone())
            .expect("valid server URL")
            .connect()
            .await
            .map_err(|e| format!("gRPC connect failed: {e}"))?;

        Ok(tonic::service::interceptor::InterceptedService::new(
            channel,
            BearerInterceptor(self.token.clone()),
        ))
    }
}

#[derive(Clone)]
struct BearerInterceptor(String);

impl tonic::service::Interceptor for BearerInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        let value = format!("Bearer {}", self.0)
            .parse()
            .map_err(|_| tonic::Status::internal("invalid token"))?;
        req.metadata_mut().insert("authorization", value);
        Ok(req)
    }
}

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
struct ProjectName {
    /// Project name (slug).
    name: String,
}

#[derive(Deserialize, JsonSchema)]
struct CreateProjectParams {
    /// Project name (slug, lowercase alphanumeric + hyphens).
    name: String,
    /// Project description.
    description: String,
    /// Visibility: "public" or "private".
    #[serde(default = "default_visibility")]
    visibility: String,
}

fn default_visibility() -> String {
    "public".to_string()
}

#[derive(Deserialize, JsonSchema)]
struct RenameProjectParams {
    /// Current project name.
    name: String,
    /// New project name.
    new_name: String,
}

#[tool_router]
impl CausesTools {
    /// Show the authenticated user's identity (user ID, display name, email,
    /// session type).
    #[tool(description = "Show the authenticated user's identity")]
    async fn whoami(&self) -> String {
        let channel = match self.authed_channel().await {
            Ok(c) => c,
            Err(e) => return format!("Connection failed: {e}"),
        };
        let mut client = AuthServiceClient::new(channel);

        match client.who_am_i(WhoAmIRequest {}).await {
            Ok(resp) => {
                let r = resp.into_inner();
                format!(
                    "User ID: {}\nDisplay name: {}\nEmail: {}\nSession: {}",
                    r.user_id,
                    r.display_name,
                    r.email,
                    if r.admin { "admin" } else { "restricted" }
                )
            }
            Err(e) => format!("WhoAmI failed: {}", e.message()),
        }
    }

    /// List all projects visible to the authenticated user.
    #[tool(description = "List all projects visible to the authenticated user")]
    async fn list_projects(&self) -> String {
        let channel = match self.authed_channel().await {
            Ok(c) => c,
            Err(e) => return format!("Connection failed: {e}"),
        };
        let mut client = ProjectServiceClient::new(channel);

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
                    .map(|p| format_project(p))
                    .collect::<Vec<_>>()
                    .join("\n---\n")
            }
            Err(e) => format!("ListProjects failed: {}", e.message()),
        }
    }

    /// Get a project by name.
    #[tool(description = "Get a project by its name (slug)")]
    async fn get_project(&self, params: Parameters<ProjectName>) -> String {
        let channel = match self.authed_channel().await {
            Ok(c) => c,
            Err(e) => return format!("Connection failed: {e}"),
        };
        let mut client = ProjectServiceClient::new(channel);

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
    async fn create_project(&self, params: Parameters<CreateProjectParams>) -> String {
        let channel = match self.authed_channel().await {
            Ok(c) => c,
            Err(e) => return format!("Connection failed: {e}"),
        };
        let mut client = ProjectServiceClient::new(channel);

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
    async fn rename_project(&self, params: Parameters<RenameProjectParams>) -> String {
        let channel = match self.authed_channel().await {
            Ok(c) => c,
            Err(e) => return format!("Connection failed: {e}"),
        };
        let mut client = ProjectServiceClient::new(channel);

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
    async fn delete_project(&self, params: Parameters<ProjectName>) -> String {
        let channel = match self.authed_channel().await {
            Ok(c) => c,
            Err(e) => return format!("Connection failed: {e}"),
        };
        let mut client = ProjectServiceClient::new(channel);

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

#[tool_handler]
impl ServerHandler for CausesTools {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use causes_proto::auth_service_server::{AuthService, AuthServiceServer};
    use causes_proto::project_service_server::{ProjectService, ProjectServiceServer};
    use causes_proto::*;

    struct MockAuthService;

    #[tonic::async_trait]
    impl AuthService for MockAuthService {
        async fn start_login(
            &self,
            _req: tonic::Request<StartLoginRequest>,
        ) -> Result<tonic::Response<StartLoginResponse>, tonic::Status> {
            unimplemented!()
        }

        async fn complete_login(
            &self,
            _req: tonic::Request<CompleteLoginRequest>,
        ) -> Result<tonic::Response<CompleteLoginResponse>, tonic::Status> {
            unimplemented!()
        }

        async fn who_am_i(
            &self,
            _req: tonic::Request<WhoAmIRequest>,
        ) -> Result<tonic::Response<WhoAmIResponse>, tonic::Status> {
            Ok(tonic::Response::new(WhoAmIResponse {
                user_id: "uid-42".to_string(),
                display_name: "Test User".to_string(),
                email: "test@example.com".to_string(),
                admin: false,
            }))
        }
    }

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

    async fn start_mock_grpc() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock = Arc::new(MockAuthService);
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(AuthServiceServer::from_arc(mock))
                .add_service(ProjectServiceServer::new(MockProjectService))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn whoami_returns_user_info() {
        let url = start_mock_grpc().await;
        let tools = CausesTools::new(url, "test-token".to_string());
        let result = tools.whoami().await;
        assert!(result.contains("uid-42"), "got: {result}");
        assert!(result.contains("Test User"));
        assert!(result.contains("test@example.com"));
        assert!(result.contains("restricted"));
    }

    #[tokio::test]
    async fn whoami_reports_connection_failure() {
        let tools = CausesTools::new("http://127.0.0.1:1".to_string(), "tok".to_string());
        let result = tools.whoami().await;
        assert!(
            result.contains("Connection failed") || result.contains("connect"),
            "got: {result}"
        );
    }

    #[tokio::test]
    async fn list_projects_returns_project_names() {
        let url = start_mock_grpc().await;
        let tools = CausesTools::new(url, "tok".to_string());
        let result = tools.list_projects().await;
        assert!(result.contains("test-project"), "got: {result}");
        assert!(result.contains("public"), "got: {result}");
    }

    #[tokio::test]
    async fn get_project_returns_details() {
        let url = start_mock_grpc().await;
        let tools = CausesTools::new(url, "tok".to_string());
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
        let url = start_mock_grpc().await;
        let tools = CausesTools::new(url, "tok".to_string());
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
        let url = start_mock_grpc().await;
        let tools = CausesTools::new(url, "tok".to_string());
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
        let url = start_mock_grpc().await;
        let tools = CausesTools::new(url, "tok".to_string());
        let result = tools
            .delete_project(Parameters(ProjectName {
                name: "test-project".to_string(),
            }))
            .await;
        assert_eq!(result, "Project deleted.");
    }
}
