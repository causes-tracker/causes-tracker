mod auth;
mod projects;
mod whoami;

use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use tonic::transport::Channel;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(super) grpc_url: String,
    pub(super) secure_cookies: bool,
}

/// Build the BFF HTTP router.
pub(crate) fn router(cfg: Arc<crate::config::Config>, grpc_url: String) -> Router {
    let secure_cookies = cfg.tls_domain.is_some();

    let state = AppState {
        grpc_url,
        secure_cookies,
    };

    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .merge(auth::routes())
        .merge(whoami::routes())
        .merge(projects::routes())
        .with_state(state)
}

async fn index() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../static/index.html"),
    )
}

async fn healthz() -> &'static str {
    "ok"
}

/// Session token extracted from the `causes_session` cookie.
///
/// Use as `Option<SessionToken>` in handler signatures: `None` means no
/// cookie was present (the user is not logged in).
#[derive(Debug)]
pub(super) struct SessionToken(pub String);

impl<S: Send + Sync> axum::extract::OptionalFromRequestParts<S> for SessionToken {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(extract_session(&parts.headers).map(SessionToken))
    }
}

/// The channel type returned by `authed_channel`, with Bearer token injection.
pub(super) type AuthedChannel =
    tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>;

/// Connect to the gRPC instance and return a channel with Bearer auth.
pub(super) async fn authed_channel(
    state: &AppState,
    session: &SessionToken,
) -> Result<AuthedChannel, (StatusCode, &'static str)> {
    let channel = Channel::from_shared(state.grpc_url.clone())
        .expect("valid gRPC URL")
        .connect()
        .await
        .map_err(|e| {
            tracing::error!("gRPC connect failed: {e}");
            (StatusCode::BAD_GATEWAY, "gRPC unavailable")
        })?;

    Ok(tonic::service::interceptor::InterceptedService::new(
        channel,
        BearerInterceptor(session.0.clone()),
    ))
}

/// Tonic interceptor that injects a Bearer token into every request.
#[derive(Clone)]
pub(super) struct BearerInterceptor(String);

impl tonic::service::Interceptor for BearerInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        let value = format!("Bearer {}", self.0)
            .parse()
            .map_err(|_| tonic::Status::internal("invalid token"))?;
        req.metadata_mut().insert("authorization", value);
        Ok(req)
    }
}

/// Map a tonic error to an appropriate HTTP status code and message.
pub(super) fn grpc_error_response(e: tonic::Status) -> impl IntoResponse {
    let status = match e.code() {
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, e.message().to_string())
}

/// Extract the `causes_session` cookie value from request headers.
fn extract_session(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("causes_session=") {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
pub(super) mod test_support {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use causes_proto::auth_service_server::{AuthService, AuthServiceServer};
    use causes_proto::project_service_server::{ProjectService, ProjectServiceServer};
    use causes_proto::*;

    /// Tokens containing this prefix are rejected by the mock with
    /// `Unauthenticated`, simulating an expired or invalid session.
    pub const REJECTED_TOKEN_PREFIX: &str = "expired";

    pub struct MockAuthService {
        poll_count: AtomicU32,
    }

    impl MockAuthService {
        pub fn new() -> Self {
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
                interval_secs: 5,
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
            req: tonic::Request<WhoAmIRequest>,
        ) -> Result<tonic::Response<WhoAmIResponse>, tonic::Status> {
            // Reject tokens starting with "expired" to test invalid-session handling.
            if let Some(auth) = req.metadata().get("authorization") {
                if auth.to_str().unwrap_or("").contains(REJECTED_TOKEN_PREFIX) {
                    return Err(tonic::Status::unauthenticated("session expired"));
                }
            }
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
                visibility: 1, // PUBLIC
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
            let resp = ListProjectsResponse {
                projects: vec![Self::test_project()],
            };
            Ok(tonic::Response::new(tokio_stream::once(Ok(resp))))
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

    /// Start a mock gRPC server and return its URL.
    pub async fn start_mock_grpc() -> String {
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

    /// Build a BFF router pointing at the given gRPC URL.
    pub fn test_router(grpc_url: &str) -> axum::Router {
        let state = super::AppState {
            grpc_url: grpc_url.to_string(),
            secure_cookies: false,
        };

        axum::Router::new()
            .route("/", axum::routing::get(super::index))
            .route("/healthz", axum::routing::get(super::healthz))
            .merge(super::auth::routes())
            .merge(super::whoami::routes())
            .merge(super::projects::routes())
            .with_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extract_session_from_single_cookie() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("causes_session=abc123"),
        );
        assert_eq!(extract_session(&headers).unwrap(), "abc123");
    }

    #[test]
    fn extract_session_from_multiple_cookies() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("other=foo; causes_session=xyz789; third=bar"),
        );
        assert_eq!(extract_session(&headers).unwrap(), "xyz789");
    }

    #[test]
    fn extract_session_returns_none_when_missing() {
        let headers = axum::http::HeaderMap::new();
        assert!(extract_session(&headers).is_none());
    }

    #[test]
    fn extract_session_returns_none_for_empty_value() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("causes_session="),
        );
        assert!(extract_session(&headers).is_none());
    }

    #[test]
    fn bearer_interceptor_sets_authorization() {
        let mut interceptor = BearerInterceptor("tok123".to_string());
        let req =
            tonic::service::Interceptor::call(&mut interceptor, tonic::Request::new(())).unwrap();
        let auth = req
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer tok123");
    }

    #[tokio::test]
    async fn session_token_extracts_from_cookie() {
        use axum::extract::OptionalFromRequestParts;

        let (mut parts, _) = axum::http::Request::builder()
            .header("cookie", "causes_session=tok456")
            .body(())
            .unwrap()
            .into_parts();
        let token = SessionToken::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(token.unwrap().0, "tok456");
    }

    #[tokio::test]
    async fn session_token_none_without_cookie() {
        use axum::extract::OptionalFromRequestParts;

        let (mut parts, _) = axum::http::Request::builder()
            .body(())
            .unwrap()
            .into_parts();
        let token = SessionToken::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn authed_channel_connects_and_injects_bearer() {
        use causes_proto::WhoAmIRequest;
        use causes_proto::auth_service_client::AuthServiceClient;

        let grpc_url = test_support::start_mock_grpc().await;
        let state = AppState {
            grpc_url,
            secure_cookies: false,
        };
        let session = SessionToken("d".repeat(64));

        let channel = authed_channel(&state, &session).await.unwrap();
        let mut client = AuthServiceClient::new(channel);

        // The mock accepts any non-"expired" token, so this should succeed.
        let resp = client
            .who_am_i(WhoAmIRequest {})
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.user_id, "uid-42");
    }

    #[tokio::test]
    async fn authed_channel_fails_on_bad_url() {
        let state = AppState {
            grpc_url: "http://127.0.0.1:1".to_string(),
            secure_cookies: false,
        };
        let session = SessionToken("tok".to_string());

        let err = authed_channel(&state, &session).await.unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_GATEWAY);
    }
}
