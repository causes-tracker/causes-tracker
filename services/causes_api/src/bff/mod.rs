mod auth;
mod whoami;

use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use causes_proto::auth_service_client::AuthServiceClient;
use tonic::transport::Channel;

#[derive(Clone)]
pub struct AppState {
    pub grpc_url: String,
    pub secure_cookies: bool,
}

/// Build the BFF HTTP router.
pub fn router(cfg: Arc<crate::config::Config>) -> Router {
    let secure_cookies = cfg.tls_domain.is_some();
    let grpc_url = if secure_cookies {
        "http://127.0.0.1:50051".to_string()
    } else {
        format!("http://{}", cfg.bind_addr)
    };

    let state = AppState {
        grpc_url,
        secure_cookies,
    };

    Router::new()
        .route("/healthz", get(healthz))
        .merge(auth::routes())
        .merge(whoami::routes())
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

/// Connect to the gRPC instance and return an authenticated client.
///
/// Extracts the `causes_session` cookie from request headers and attaches
/// it as a Bearer token via a tonic interceptor.
pub async fn grpc_client(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<
    AuthServiceClient<tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>>,
    (StatusCode, &'static str),
> {
    let token = extract_session(headers).ok_or((StatusCode::UNAUTHORIZED, "not logged in"))?;

    let channel = Channel::from_shared(state.grpc_url.clone())
        .expect("valid gRPC URL")
        .connect()
        .await
        .map_err(|e| {
            tracing::error!("gRPC connect failed: {e}");
            (StatusCode::BAD_GATEWAY, "gRPC unavailable")
        })?;

    Ok(AuthServiceClient::with_interceptor(
        channel,
        BearerInterceptor(token),
    ))
}

/// Tonic interceptor that injects a Bearer token into every request.
#[derive(Clone)]
pub struct BearerInterceptor(String);

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
pub fn grpc_error_response(e: tonic::Status) -> impl IntoResponse {
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
