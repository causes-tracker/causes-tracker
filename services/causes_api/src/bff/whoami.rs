use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Serialize;

use causes_proto::WhoAmIRequest;

use super::{AppState, grpc_client, grpc_error_response};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/whoami", get(api_whoami))
}

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
    let mut client = match grpc_client(&state, &headers).await {
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
            })
            .into_response()
        }
        Err(e) => grpc_error_response(e).into_response(),
    }
}
