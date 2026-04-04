use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::{
    CompleteLoginRequest, StartLoginRequest, WhoAmIRequest, complete_login_response,
};

#[derive(Clone)]
struct AppState {
    grpc_url: String,
    secure_cookies: bool,
}

/// Build the BFF HTTP router.
pub fn router(cfg: Arc<crate::config::Config>) -> Router {
    let secure_cookies = cfg.tls_domain.is_some();
    let grpc_url = if secure_cookies {
        // In TLS mode, the loopback gRPC listener is always on 127.0.0.1:50051.
        "http://127.0.0.1:50051".to_string()
    } else {
        // In dev mode, gRPC shares the same listener.
        format!("http://{}", cfg.bind_addr)
    };

    let state = AppState {
        grpc_url,
        secure_cookies,
    };

    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/auth/login", post(auth_login))
        .route("/auth/poll", post(auth_poll))
        .route("/auth/logout", post(auth_logout))
        .route("/api/whoami", get(api_whoami))
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

// ── Auth types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct LoginResponse {
    nonce: String,
    user_code: String,
    verification_url: String,
    interval_secs: u32,
}

#[derive(Deserialize)]
struct PollRequest {
    nonce: String,
}

#[derive(Serialize)]
struct PollResponse {
    status: &'static str,
}

// ── Auth handlers ────────────────────────────────────────────────────────

async fn auth_login(State(state): State<AppState>) -> impl IntoResponse {
    let mut client = match AuthServiceClient::connect(state.grpc_url).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("gRPC connect failed: {e}");
            return (StatusCode::BAD_GATEWAY, "gRPC unavailable").into_response();
        }
    };

    let resp = match client.start_login(StartLoginRequest {}).await {
        Ok(r) => r.into_inner(),
        Err(e) => {
            tracing::error!("StartLogin failed: {e}");
            return (StatusCode::BAD_GATEWAY, "StartLogin failed").into_response();
        }
    };

    axum::Json(LoginResponse {
        nonce: resp.nonce,
        user_code: resp.user_code,
        verification_url: resp.verification_url,
        interval_secs: resp.interval_secs as u32,
    })
    .into_response()
}

async fn auth_poll(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<PollRequest>,
) -> impl IntoResponse {
    let mut client = match AuthServiceClient::connect(state.grpc_url).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("gRPC connect failed: {e}");
            return (StatusCode::BAD_GATEWAY, "gRPC unavailable").into_response();
        }
    };

    let resp = match client
        .complete_login(CompleteLoginRequest {
            nonce: body.nonce,
            admin: false,
        })
        .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => {
            tracing::error!("CompleteLogin failed: {e}");
            return (StatusCode::BAD_GATEWAY, "CompleteLogin failed").into_response();
        }
    };

    match resp.result {
        Some(complete_login_response::Result::Pending(_)) => {
            axum::Json(PollResponse { status: "pending" }).into_response()
        }
        Some(complete_login_response::Result::SessionCreated(sc)) => {
            let secure = if state.secure_cookies { "; Secure" } else { "" };
            let cookie = format!(
                "causes_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000{}",
                sc.session_token, secure,
            );
            (
                [(axum::http::header::SET_COOKIE, cookie)],
                axum::Json(PollResponse { status: "ok" }),
            )
                .into_response()
        }
        None => (StatusCode::BAD_GATEWAY, "unexpected empty response").into_response(),
    }
}

async fn auth_logout(State(state): State<AppState>) -> impl IntoResponse {
    let secure = if state.secure_cookies { "; Secure" } else { "" };
    let cookie = format!(
        "causes_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{}",
        secure,
    );
    (
        [(axum::http::header::SET_COOKIE, cookie)],
        axum::Json(PollResponse { status: "ok" }),
    )
}

// ── Proxy endpoints ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct WhoAmIResponse {
    user_id: String,
    display_name: String,
    email: String,
    admin: bool,
}

async fn api_whoami(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let token = match extract_session(&headers) {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    };

    let mut client = match AuthServiceClient::connect(state.grpc_url).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("gRPC connect failed: {e}");
            return (StatusCode::BAD_GATEWAY, "gRPC unavailable").into_response();
        }
    };

    let mut req = tonic::Request::new(WhoAmIRequest {});
    req.metadata_mut().insert(
        "authorization",
        match format!("Bearer {token}").parse() {
            Ok(v) => v,
            Err(_) => return (StatusCode::UNAUTHORIZED, "invalid session").into_response(),
        },
    );

    match client.who_am_i(req).await {
        Ok(resp) => {
            let inner = resp.into_inner();
            axum::Json(WhoAmIResponse {
                user_id: inner.user_id,
                display_name: inner.display_name,
                email: inner.email,
                admin: inner.admin,
            })
            .into_response()
        }
        Err(e) => {
            let status = match e.code() {
                tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
                _ => StatusCode::BAD_GATEWAY,
            };
            (status, e.message().to_string()).into_response()
        }
    }
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
