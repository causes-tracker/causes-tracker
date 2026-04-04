use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Serialize;

use causes_proto::WhoAmIRequest;

use super::{AppState, SessionToken, grpc_client, grpc_error_response};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/whoami", get(api_whoami))
}

#[derive(Serialize)]
struct WhoAmIResponse {
    user_id: String,
    display_name: String,
    email: String,
    admin: bool,
    logged_in: bool,
}

async fn api_whoami(
    State(state): State<AppState>,
    session: Option<SessionToken>,
) -> impl IntoResponse {
    let session = match session {
        Some(s) => s,
        None => {
            return axum::Json(WhoAmIResponse {
                user_id: String::new(),
                display_name: String::new(),
                email: String::new(),
                admin: false,
                logged_in: false,
            })
            .into_response();
        }
    };

    let mut client = match grpc_client(&state, &session).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };

    match client.who_am_i(WhoAmIRequest {}).await {
        Ok(resp) => {
            let inner = resp.into_inner();
            axum::Json(WhoAmIResponse {
                user_id: inner.user_id,
                display_name: inner.display_name,
                email: inner.email,
                admin: inner.admin,
                logged_in: true,
            })
            .into_response()
        }
        Err(e) if e.code() == tonic::Code::Unauthenticated => axum::Json(WhoAmIResponse {
            user_id: String::new(),
            display_name: String::new(),
            email: String::new(),
            admin: false,
            logged_in: false,
        })
        .into_response(),
        Err(e) => grpc_error_response(e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::super::test_support::{REJECTED_TOKEN_PREFIX, start_mock_grpc, test_router};

    #[tokio::test]
    async fn whoami_returns_user_info_with_valid_cookie() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/whoami")
                    .header("cookie", format!("causes_session={}", "d".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["user_id"], "uid-42");
        assert_eq!(json["display_name"], "Test User");
        assert_eq!(json["email"], "test@example.com");
        assert_eq!(json["admin"], false);
        assert_eq!(json["logged_in"], true);
    }

    #[tokio::test]
    async fn whoami_returns_not_logged_in_without_cookie() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/whoami")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["logged_in"], false);
    }

    #[tokio::test]
    async fn whoami_returns_not_logged_in_for_expired_session() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/whoami")
                    .header(
                        "cookie",
                        format!("causes_session={REJECTED_TOKEN_PREFIX}_token"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["logged_in"], false);
    }
}
