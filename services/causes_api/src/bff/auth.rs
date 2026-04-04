use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::{Deserialize, Serialize};

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::{CompleteLoginRequest, StartLoginRequest, complete_login_response};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(auth_login))
        .route("/auth/poll", post(auth_poll))
        .route("/auth/logout", post(auth_logout))
}

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
