//! MCP server for Causes — exposes bug tracker tools to AI assistants.
//!
//! Transport-agnostic: callers bind this to stdio (CLI) or Streamable HTTP (BFF).

mod session;

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;

use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::Deserialize;

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::project_service_client::ProjectServiceClient;
use causes_proto::{
    CompleteLoginRequest, CreateProjectRequest, DeleteProjectRequest, GetProjectRequest,
    ListProjectsRequest, RenameProjectRequest, StartLoginRequest, WhoAmIRequest,
    complete_login_response,
};
use proto_ext::BearerInterceptor;
use tonic::transport::Channel;

type AuthedChannel = tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>;

/// MCP server exposing Causes bug tracker tools.
///
/// Transport-agnostic: callers provide stdio or HTTP transport.
/// The server connects to a Causes gRPC API using a session token
/// obtained via the `login` tool.
#[derive(Clone)]
pub struct CausesTools {
    server_url: String,
    data_dir: std::path::PathBuf,
    channel: Channel,
    #[allow(dead_code)] // read by #[tool_handler] macro expansion
    tool_router: ToolRouter<Self>,
}

impl CausesTools {
    pub fn new(server_url: String, data_dir: std::path::PathBuf) -> Self {
        let channel = Channel::from_shared(server_url.clone())
            .expect("valid server URL")
            .connect_lazy();
        Self {
            server_url,
            data_dir,
            channel,
            tool_router: Self::tool_router(),
        }
    }

    fn authed_channel(&self) -> Result<AuthedChannel, String> {
        let token = session::load(&self.data_dir, &self.server_url)
            .map_err(|e| format!("Failed to read session: {e}"))?
            .ok_or_else(|| "Not logged in. Use the login tool first.".to_string())?;

        Ok(tonic::service::interceptor::InterceptedService::new(
            self.channel.clone(),
            BearerInterceptor::new(token),
        ))
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
    /// Visibility: "public" or "private". Defaults to "public".
    #[serde(default = "default_public")]
    visibility: String,
}

fn default_public() -> String {
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
    /// Log in to the Causes instance.  Starts the device authorization flow,
    /// shows the user a verification URL and code, then waits for them to
    /// complete sign-in in their browser.  The session is stored automatically.
    #[tool(
        description = "Log in to the Causes instance via device authorization flow. Returns a URL and code for the user to complete in their browser, then waits for completion."
    )]
    async fn login(&self) -> String {
        // StartLogin is unauthenticated — use the plain channel.
        let mut client = AuthServiceClient::new(self.channel.clone());

        let resp = match client.start_login(StartLoginRequest {}).await {
            Ok(r) => r.into_inner(),
            Err(e) => return format!("StartLogin failed: {}", e.message()),
        };

        let user_code = resp.user_code.clone();
        let verification_url = resp.verification_url.clone();
        let interval = std::time::Duration::from_secs(resp.interval_secs.max(1) as u64);

        // Long-poll until the user completes sign-in.
        loop {
            tokio::time::sleep(interval).await;

            let poll_resp = match client
                .complete_login(CompleteLoginRequest {
                    nonce: resp.nonce.clone(),
                    admin: false,
                })
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return format!("CompleteLogin failed: {}", e.message()),
            };

            match poll_resp.result {
                Some(complete_login_response::Result::Pending(_)) => continue,
                Some(complete_login_response::Result::SessionCreated(sc)) => {
                    if let Err(e) =
                        session::save(&self.data_dir, &self.server_url, &sc.session_token)
                    {
                        return format!("Login succeeded but failed to save session: {e}");
                    }
                    return format!(
                        "Login successful. Session saved.\n\
                         (User signed in via {verification_url} with code {user_code})"
                    );
                }
                None => return "Unexpected empty response from server.".to_string(),
            }
        }
    }

    /// Show the authenticated user's identity (user ID, display name, email,
    /// session type).
    #[tool(description = "Show the authenticated user's identity")]
    async fn whoami(&self) -> String {
        let mut client = match self.authed_channel() {
            Ok(c) => AuthServiceClient::new(c),
            Err(e) => return e,
        };

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
        let mut client = match self.authed_channel() {
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
        let mut client = match self.authed_channel() {
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
    async fn create_project(&self, params: Parameters<CreateProjectParams>) -> String {
        let mut client = match self.authed_channel() {
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
    async fn rename_project(&self, params: Parameters<RenameProjectParams>) -> String {
        let mut client = match self.authed_channel() {
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
    async fn delete_project(&self, params: Parameters<ProjectName>) -> String {
        let mut client = match self.authed_channel() {
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

// #[tool_handler] generates the ServerHandler method implementations by
// delegating tool dispatch to the ToolRouter populated by #[tool_router] above.
// The body is empty because all behaviour comes from the macro expansion.
#[tool_handler]
impl ServerHandler for CausesTools {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use causes_proto::auth_service_server::{AuthService, AuthServiceServer};
    use causes_proto::project_service_server::{ProjectService, ProjectServiceServer};
    use causes_proto::*;

    struct MockAuthService {
        poll_count: AtomicU32,
    }

    impl MockAuthService {
        fn new() -> Self {
            Self {
                poll_count: AtomicU32::new(0),
            }
        }
    }

    #[tonic::async_trait]
    impl AuthService for MockAuthService {
        async fn start_login(
            &self,
            _req: tonic::Request<StartLoginRequest>,
        ) -> Result<tonic::Response<StartLoginResponse>, tonic::Status> {
            Ok(tonic::Response::new(StartLoginResponse {
                nonce: "a".repeat(64),
                user_code: "TEST-CODE".to_string(),
                verification_url: "https://example.com/device".to_string(),
                interval_secs: 1,
            }))
        }

        async fn complete_login(
            &self,
            _req: tonic::Request<CompleteLoginRequest>,
        ) -> Result<tonic::Response<CompleteLoginResponse>, tonic::Status> {
            let n = self.poll_count.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Ok(tonic::Response::new(CompleteLoginResponse {
                    result: Some(complete_login_response::Result::Pending(Pending {})),
                }))
            } else {
                Ok(tonic::Response::new(CompleteLoginResponse {
                    result: Some(complete_login_response::Result::SessionCreated(
                        SessionCreated {
                            session_token: "d".repeat(64),
                        },
                    )),
                }))
            }
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

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("causes-mcp-{name}-{}", std::process::id()))
    }

    async fn start_mock_grpc() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock = Arc::new(MockAuthService::new());
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
    async fn login_completes_and_saves_session() {
        let url = start_mock_grpc().await;
        let dir = test_dir("login");
        let tools = CausesTools::new(url.clone(), dir.clone());

        let result = tools.login().await;
        assert!(result.contains("Login successful"), "got: {result}");
        assert!(result.contains("TEST-CODE"));

        // Session should be saved.
        let token = session::load(&dir, &url).unwrap().unwrap();
        assert_eq!(token, "d".repeat(64));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn whoami_requires_login() {
        let url = start_mock_grpc().await;
        let dir = test_dir("whoami-no-login");
        let tools = CausesTools::new(url, dir.clone());

        let result = tools.whoami().await;
        assert!(result.contains("Not logged in"), "got: {result}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn whoami_works_after_login() {
        let url = start_mock_grpc().await;
        let dir = test_dir("whoami-after-login");
        let tools = CausesTools::new(url, dir.clone());

        // Login first.
        let login_result = tools.login().await;
        assert!(
            login_result.contains("Login successful"),
            "got: {login_result}"
        );

        // Now whoami should work.
        let result = tools.whoami().await;
        assert!(result.contains("uid-42"), "got: {result}");
        assert!(result.contains("Test User"));
        assert!(result.contains("restricted"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn whoami_reports_connection_failure() {
        let dir = test_dir("whoami-fail");
        // Save a fake token so it doesn't fail on "not logged in".
        session::save(&dir, "http://127.0.0.1:1", "fake-token").unwrap();
        let tools = CausesTools::new("http://127.0.0.1:1".to_string(), dir.clone());

        let result = tools.whoami().await;
        assert!(result.contains("failed"), "got: {result}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Helper: create tools and log in via the mock.
    async fn logged_in_tools(name: &str) -> (CausesTools, std::path::PathBuf) {
        let url = start_mock_grpc().await;
        let dir = test_dir(name);
        let tools = CausesTools::new(url, dir.clone());
        let result = tools.login().await;
        assert!(result.contains("Login successful"), "login got: {result}");
        (tools, dir)
    }

    #[tokio::test]
    async fn list_projects_returns_projects() {
        let (tools, dir) = logged_in_tools("list-projects").await;
        let result = tools.list_projects().await;
        assert!(result.contains("test-project"), "got: {result}");
        assert!(result.contains("public"), "got: {result}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn get_project_returns_details() {
        let (tools, dir) = logged_in_tools("get-project").await;
        let result = tools
            .get_project(Parameters(ProjectName {
                name: "test-project".to_string(),
            }))
            .await;
        assert!(result.contains("test-project"), "got: {result}");
        assert!(result.contains("A test project"), "got: {result}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn create_project_returns_created() {
        let (tools, dir) = logged_in_tools("create-project").await;
        let result = tools
            .create_project(Parameters(CreateProjectParams {
                name: "new-project".to_string(),
                description: "desc".to_string(),
                visibility: "public".to_string(),
            }))
            .await;
        assert!(result.contains("Created project"), "got: {result}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn rename_project_returns_renamed() {
        let (tools, dir) = logged_in_tools("rename-project").await;
        let result = tools
            .rename_project(Parameters(RenameProjectParams {
                name: "old".to_string(),
                new_name: "new".to_string(),
            }))
            .await;
        assert!(result.contains("Renamed project"), "got: {result}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn delete_project_returns_deleted() {
        let (tools, dir) = logged_in_tools("delete-project").await;
        let result = tools
            .delete_project(Parameters(ProjectName {
                name: "test-project".to_string(),
            }))
            .await;
        assert_eq!(result, "Project deleted.");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn project_tools_require_login() {
        let url = start_mock_grpc().await;
        let dir = test_dir("projects-no-login");
        let tools = CausesTools::new(url, dir.clone());

        let result = tools.list_projects().await;
        assert!(result.contains("Not logged in"), "got: {result}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
