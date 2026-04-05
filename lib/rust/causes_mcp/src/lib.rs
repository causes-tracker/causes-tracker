use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;

use causes_proto::WhoAmIRequest;
use causes_proto::auth_service_client::AuthServiceClient;
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
}

#[tool_handler]
impl ServerHandler for CausesTools {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use causes_proto::auth_service_server::{AuthService, AuthServiceServer};
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

    async fn start_mock_grpc() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock = Arc::new(MockAuthService);
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
}
