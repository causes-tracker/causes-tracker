use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::{Deserialize, Serialize};

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::{CompleteLoginRequest, StartLoginRequest, complete_login_response};

use super::AppState;

pub(super) fn routes() -> Router<AppState> {
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::super::test_support::{start_mock_grpc, test_router};

    #[tokio::test]
    async fn login_returns_device_code() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["user_code"], "TEST-CODE");
        assert_eq!(json["verification_url"], "https://example.com/device");
        assert_eq!(json["interval_secs"], 5);
        assert_eq!(json["nonce"].as_str().unwrap().len(), 64);
    }

    #[tokio::test]
    async fn poll_returns_pending_then_session_cookie() {
        let grpc_url = start_mock_grpc().await;

        // First poll: pending
        let app = test_router(&grpc_url);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/poll")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"nonce": "a".repeat(64)}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "pending");

        // Second poll: session created
        let app = test_router(&grpc_url);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/poll")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"nonce": "a".repeat(64)}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(cookie.starts_with(&format!("causes_session={}", "d".repeat(64))));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
    }

    #[tokio::test]
    async fn logout_clears_cookie() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(cookie.contains("causes_session=;"));
        assert!(cookie.contains("Max-Age=0"));
    }
}
