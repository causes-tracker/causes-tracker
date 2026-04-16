//! MCP server for Causes — exposes project tracker tools to AI assistants.
//!
//! Transport-agnostic: callers bind this to stdio (CLI) or Streamable HTTP (BFF).

mod project;

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;

use std::sync::Arc;

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::{
    CompleteLoginRequest, StartLoginRequest, WhoAmIRequest, complete_login_response,
};
use proto_ext::BearerInterceptor;
use tokio::sync::RwLock;
use tonic::transport::Channel;
use tracing::debug;

pub(crate) type AuthedChannel =
    tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>;

/// State saved between the first login call (which returns the URL/code)
/// and the second call (which polls for completion).
#[derive(Clone)]
struct PendingLogin {
    nonce: String,
    user_code: String,
    verification_url: String,
    interval: std::time::Duration,
}

/// MCP server exposing Causes project tracker tools.
///
/// Transport-agnostic: callers provide stdio or HTTP transport.
/// The server connects to a Causes gRPC API using a session token
/// obtained via the `login` tool.
#[derive(Clone)]
pub struct CausesTools {
    server_url: String,
    session: Arc<dyn causes_session::SessionStorage>,
    channel: Channel,
    cached_channel: Arc<RwLock<Option<AuthedChannel>>>,
    pending_login: Arc<RwLock<Option<PendingLogin>>>,
    login_timeout: std::time::Duration,
    #[allow(dead_code)] // read by #[tool_handler] macro expansion
    tool_router: ToolRouter<Self>,
}

impl CausesTools {
    pub fn new(server_url: String, data_dir: std::path::PathBuf) -> Self {
        let tls = server_url.starts_with("https://");
        debug!(server_url, tls, ?data_dir, "creating MCP handler");
        let mut endpoint = Channel::from_shared(server_url.clone()).expect("valid server URL");
        if tls {
            endpoint = endpoint
                .tls_config(tonic::transport::ClientTlsConfig::new().with_native_roots())
                .expect("TLS config");
        }
        let session = causes_session::FileSessionStore::new(
            causes_session::SessionKind::Mcp,
            &data_dir,
            &server_url,
        );
        Self::with_channel(server_url, Arc::new(session), endpoint.connect_lazy())
    }

    pub(crate) fn with_channel(
        server_url: String,
        session: Arc<dyn causes_session::SessionStorage>,
        channel: Channel,
    ) -> Self {
        Self {
            server_url,
            session,
            channel,
            cached_channel: Arc::new(RwLock::new(None)),
            pending_login: Arc::new(RwLock::new(None)),
            login_timeout: std::time::Duration::from_secs(600),
            tool_router: Self::tool_router() + Self::project_router(),
        }
    }

    async fn authed_client(&self) -> Result<AuthServiceClient<AuthedChannel>, String> {
        Ok(AuthServiceClient::new(self.authed_channel().await?))
    }

    pub(crate) async fn authed_channel(&self) -> Result<AuthedChannel, String> {
        if let Some(ch) = self.cached_channel.read().await.clone() {
            debug!("using cached authed channel");
            return Ok(ch);
        }

        debug!("loading session from disk");
        let token = self
            .session
            .load()
            .map_err(|e| format!("Failed to read session: {e}"))?
            .ok_or_else(|| "Not logged in. Use the login tool first.".to_string())?;
        debug!(token_len = token.len(), "loaded session token");

        let ch = tonic::service::interceptor::InterceptedService::new(
            self.channel.clone(),
            BearerInterceptor::new(token),
        );
        *self.cached_channel.write().await = Some(ch.clone());
        Ok(ch)
    }

    pub(crate) async fn set_authed_channel(&self, token: &str) {
        let ch = tonic::service::interceptor::InterceptedService::new(
            self.channel.clone(),
            BearerInterceptor::new(token.to_string()),
        );
        *self.cached_channel.write().await = Some(ch);
    }

    /// Poll for completion of a pending device-auth login.
    async fn poll_login(&self, pending: &PendingLogin) -> String {
        let mut client = AuthServiceClient::new(self.channel.clone());
        let deadline = tokio::time::Instant::now() + self.login_timeout;
        let mut attempt = 0u32;

        loop {
            tokio::time::sleep(pending.interval).await;

            if tokio::time::Instant::now() >= deadline {
                debug!(?self.login_timeout, "login poll timed out");
                *self.pending_login.write().await = None;
                return format!(
                    "Login timed out after {} minutes.",
                    self.login_timeout.as_secs() / 60
                );
            }

            attempt += 1;
            debug!(attempt, "polling CompleteLogin");

            let poll_resp = match client
                .complete_login(CompleteLoginRequest {
                    nonce: pending.nonce.clone(),
                    admin: false,
                })
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => {
                    debug!(error = e.message(), "CompleteLogin failed");
                    *self.pending_login.write().await = None;
                    return format!("CompleteLogin failed: {}", e.message());
                }
            };

            match poll_resp.result {
                Some(complete_login_response::Result::Pending(_)) => continue,
                Some(complete_login_response::Result::SessionCreated(sc)) => {
                    // Drop the pending state before taking more locks.
                    *self.pending_login.write().await = None;
                    if let Err(e) = self.session.save(&sc.session_token) {
                        return format!("Login succeeded but failed to save session: {e}");
                    }
                    self.set_authed_channel(&sc.session_token).await;
                    debug!("login complete");
                    return format!(
                        "Login successful. Session saved.\n\
                         (User signed in via {} with code {})",
                        pending.verification_url, pending.user_code
                    );
                }
                None => {
                    *self.pending_login.write().await = None;
                    return "Unexpected empty response from server.".to_string();
                }
            }
        }
    }
}

#[tool_router]
impl CausesTools {
    /// Log in to the Causes instance via device authorization flow.
    ///
    /// First call: starts the flow and returns a verification URL and code.
    /// Second call: polls the server until sign-in completes (long poll).
    #[tool(
        description = "Log in to the Causes instance. First call returns a URL and code for the user to visit. Call again to wait for login completion."
    )]
    async fn login(&self) -> String {
        debug!(server = self.server_url.as_str(), "login called");

        // If we already have a valid session, skip the login flow.
        if let Ok(mut client) = self.authed_client().await {
            match client.who_am_i(WhoAmIRequest {}).await {
                Ok(resp) => {
                    let r = resp.into_inner();
                    debug!(email = r.email.as_str(), "already logged in");
                    return format!("Already logged in as {} ({}).", r.display_name, r.email);
                }
                Err(e) => {
                    debug!(error = e.message(), "saved session rejected");
                    *self.cached_channel.write().await = None;
                }
            }
        }

        // If there's a pending login from a previous call, poll for completion.
        // Clone and drop the read guard before calling poll_login, which
        // needs a write lock on pending_login.
        let pending = self.pending_login.read().await.clone();
        if let Some(pending) = pending {
            debug!("resuming pending login");
            return self.poll_login(&pending).await;
        }

        // No session, no pending login — start a new device auth flow.
        let mut client = AuthServiceClient::new(self.channel.clone());

        debug!("calling StartLogin");
        let resp = match client.start_login(StartLoginRequest {}).await {
            Ok(r) => r.into_inner(),
            Err(e) => {
                debug!(error = e.message(), "StartLogin failed");
                return format!("StartLogin failed: {}", e.message());
            }
        };

        let pending = PendingLogin {
            nonce: resp.nonce.clone(),
            user_code: resp.user_code.clone(),
            verification_url: resp.verification_url.clone(),
            interval: std::time::Duration::from_secs(resp.interval_secs.max(1) as u64),
        };
        debug!(
            user_code = pending.user_code.as_str(),
            url = pending.verification_url.as_str(),
            interval_secs = pending.interval.as_secs(),
            "device auth flow started"
        );

        *self.pending_login.write().await = Some(pending.clone());

        // Return immediately so the MCP client sees the URL and code.
        format!(
            "Login started. Please visit {} and enter code: {}\n\
             Then call this tool again to complete login.",
            pending.verification_url, pending.user_code
        )
    }

    /// Show the authenticated user's identity (user ID, display name, email,
    /// session type).
    #[tool(description = "Show the authenticated user's identity")]
    async fn whoami(&self) -> String {
        let mut client = match self.authed_client().await {
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
#[tool_handler]
impl ServerHandler for CausesTools {
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        // Manually implemented because the `#[tool_handler]` macro's generated
        // `list_tools` uses the first `#[tool_router]`-generated router only,
        // not the merged `self.tool_router` field.  Using the field directly
        // includes both login/whoami and project tools.
        Ok(rmcp::model::ListToolsResult {
            tools: self.tool_router.list_all(),
            meta: None,
            next_cursor: None,
        })
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        let mut info = rmcp::model::ServerInfo::default();
        info.server_info = rmcp::model::Implementation::new("causes", env!("CARGO_PKG_VERSION"));
        info.capabilities = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build();
        info.instructions = Some(
            "Causes project tracker. Use the login tool to authenticate, \
             then use project tools to manage projects."
                .to_string(),
        );
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use causes_proto::auth_service_server::{AuthService, AuthServiceServer};
    use causes_proto::*;
    use causes_session::SessionStorage;

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

    /// Mock that always returns Pending — login never completes.
    /// Only used by the timeout test in this module.
    struct AlwaysPendingAuthService;

    #[tonic::async_trait]
    impl AuthService for AlwaysPendingAuthService {
        async fn start_login(
            &self,
            _req: tonic::Request<StartLoginRequest>,
        ) -> Result<tonic::Response<StartLoginResponse>, tonic::Status> {
            Ok(tonic::Response::new(StartLoginResponse {
                nonce: "b".repeat(64),
                user_code: "PEND-CODE".to_string(),
                verification_url: "https://example.com/device".to_string(),
                interval_secs: 1,
            }))
        }

        async fn complete_login(
            &self,
            _req: tonic::Request<CompleteLoginRequest>,
        ) -> Result<tonic::Response<CompleteLoginResponse>, tonic::Status> {
            Ok(tonic::Response::new(CompleteLoginResponse {
                result: Some(complete_login_response::Result::Pending(Pending {})),
            }))
        }

        async fn who_am_i(
            &self,
            _req: tonic::Request<WhoAmIRequest>,
        ) -> Result<tonic::Response<WhoAmIResponse>, tonic::Status> {
            Err(tonic::Status::unauthenticated("no"))
        }
    }

    // ── Login tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn login_first_call_returns_url_and_code() {
        let url = start_mock_grpc().await;
        let dir = tempfile::tempdir().unwrap();
        let tools = CausesTools::new(url, dir.path().to_path_buf());

        let result = tools.login().await;
        assert!(result.contains("Login started"), "got: {result}");
        assert!(result.contains("TEST-CODE"), "got: {result}");
        assert!(result.contains("example.com/device"), "got: {result}");
        assert!(result.contains("call this tool again"), "got: {result}");
    }

    #[tokio::test]
    async fn login_second_call_polls_and_completes() {
        let url = start_mock_grpc().await;
        let dir = tempfile::tempdir().unwrap();
        let tools = CausesTools::new(url.clone(), dir.path().to_path_buf());

        tools.login().await;
        let second = tools.login().await;
        assert!(second.contains("Login successful"), "got: {second}");

        let store = causes_session::FileSessionStore::new(
            causes_session::SessionKind::Mcp,
            dir.path(),
            &url,
        );
        assert_eq!(store.load().unwrap().unwrap(), "d".repeat(64));
    }

    #[tokio::test]
    async fn login_skips_when_already_logged_in() {
        let url = start_mock_grpc().await;
        let dir = tempfile::tempdir().unwrap();
        let tools = CausesTools::new(url, dir.path().to_path_buf());
        tools.login().await;
        tools.login().await;

        let result = tools.login().await;
        assert!(result.contains("Already logged in"), "got: {result}");
        assert!(result.contains("Test User"), "got: {result}");
    }

    #[tokio::test]
    async fn login_times_out_when_never_completed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(AuthServiceServer::new(AlwaysPendingAuthService))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let dir = tempfile::tempdir().unwrap();
        let mut tools =
            CausesTools::new(format!("http://127.0.0.1:{port}"), dir.path().to_path_buf());
        tools.login_timeout = std::time::Duration::from_secs(3);

        let first = tools.login().await;
        assert!(first.contains("Login started"), "got: {first}");

        let second = tools.login().await;
        assert!(second.contains("timed out"), "got: {second}");
    }

    // ── Whoami tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn whoami_requires_login() {
        let url = start_mock_grpc().await;
        let dir = tempfile::tempdir().unwrap();
        let tools = CausesTools::new(url, dir.path().to_path_buf());

        let result = tools.whoami().await;
        assert!(result.contains("Not logged in"), "got: {result}");
    }

    #[tokio::test]
    async fn whoami_works_after_login() {
        let url = start_mock_grpc().await;
        let dir = tempfile::tempdir().unwrap();
        let tools = CausesTools::new(url, dir.path().to_path_buf());
        tools.login().await;
        tools.login().await;

        let result = tools.whoami().await;
        assert!(result.contains("uid-42"), "got: {result}");
        assert!(result.contains("Test User"));
        assert!(result.contains("restricted"));
    }

    #[tokio::test]
    async fn whoami_reports_connection_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = causes_session::FileSessionStore::new(
            causes_session::SessionKind::Mcp,
            dir.path(),
            "http://127.0.0.1:1",
        );
        store.save(&"a".repeat(64)).unwrap();
        let tools = CausesTools::new("http://127.0.0.1:1".to_string(), dir.path().to_path_buf());

        let result = tools.whoami().await;
        assert!(result.contains("failed"), "got: {result}");
    }

    // ── TLS tests ───────────────────────────────────────────────────────────

    async fn start_mock_grpc_tls() -> (String, tonic::transport::Certificate) {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let key_pair = rcgen::KeyPair::generate().expect("keygen");
        let ca = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .expect("cert params")
            .self_signed(&key_pair)
            .expect("self-sign");

        let server_tls = tonic::transport::ServerTlsConfig::new().identity(
            tonic::transport::Identity::from_pem(ca.pem(), key_pair.serialize_pem()),
        );
        let ca_cert = tonic::transport::Certificate::from_pem(ca.pem());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock = Arc::new(MockAuthService::new());
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(server_tls)
                .expect("server TLS config")
                .add_service(AuthServiceServer::from_arc(mock))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (format!("https://localhost:{port}"), ca_cert)
    }

    #[tokio::test]
    async fn https_without_tls_config_fails_fast() {
        let (url, _ca_cert) = start_mock_grpc_tls().await;
        let dir = tempfile::tempdir().unwrap();

        let channel = Channel::from_shared(url.clone()).unwrap().connect_lazy();
        let session = causes_session::FileSessionStore::new(
            causes_session::SessionKind::Mcp,
            dir.path(),
            &url,
        );
        let tools = CausesTools::with_channel(url, Arc::new(session), channel);

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), tools.login())
            .await
            .expect("should not hang");

        assert!(
            result.contains("failed") || result.contains("error") || result.contains("TLS"),
            "expected TLS failure, got: {result}"
        );
    }

    #[tokio::test]
    async fn login_works_over_tls() {
        let (url, ca_cert) = start_mock_grpc_tls().await;
        let dir = tempfile::tempdir().unwrap();

        let channel = Channel::from_shared(url.clone())
            .unwrap()
            .tls_config(
                tonic::transport::ClientTlsConfig::new()
                    .ca_certificate(ca_cert)
                    .domain_name("localhost"),
            )
            .unwrap()
            .connect_lazy();

        let session = causes_session::FileSessionStore::new(
            causes_session::SessionKind::Mcp,
            dir.path(),
            &url,
        );
        let mut tools = CausesTools::with_channel(url.clone(), Arc::new(session), channel);
        tools.login_timeout = std::time::Duration::from_secs(10);

        let first = tokio::time::timeout(std::time::Duration::from_secs(5), tools.login())
            .await
            .expect("first login should not hang");
        assert!(first.contains("Login started"), "got: {first}");

        let second = tokio::time::timeout(std::time::Duration::from_secs(5), tools.login())
            .await
            .expect("second login should not hang");
        assert!(second.contains("Login successful"), "got: {second}");

        let store = causes_session::FileSessionStore::new(
            causes_session::SessionKind::Mcp,
            dir.path(),
            &url,
        );
        assert_eq!(store.load().unwrap().unwrap(), "d".repeat(64));
    }
}
