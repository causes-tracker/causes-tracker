use std::sync::Arc;

use axum::Router;
use axum::routing::get;

/// Build the BFF HTTP router.
pub fn router(_cfg: Arc<crate::config::Config>) -> Router {
    Router::new().route("/healthz", get(healthz))
}

async fn healthz() -> &'static str {
    "ok"
}
