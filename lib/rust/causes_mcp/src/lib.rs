//! MCP server for Causes — exposes bug tracker tools to AI assistants.
//!
//! Transport-agnostic: callers bind this to stdio (CLI) or Streamable HTTP (BFF).

mod session;

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::{
    CompleteLoginRequest, StartLoginRequest, WhoAmIRequest, complete_login_response,
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

    fn authed_client(&self) -> Result<AuthServiceClient<AuthedChannel>, String> {
        let token = session::load(&self.data_dir, &self.server_url)
            .map_err(|e| format!("Failed to read session: {e}"))?
            .ok_or_else(|| "Not logged in. Use the login tool first.".to_string())?;

        Ok(AuthServiceClient::with_interceptor(
            self.channel.clone(),
            BearerInterceptor::new(token),
        ))
    }
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
        let mut client = match self.authed_client() {
            Ok(c) => c,
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
}
