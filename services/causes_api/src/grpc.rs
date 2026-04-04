use std::sync::Arc;

use tonic_health::ServingStatus;
use tonic_health::pb::health_server::{Health, HealthServer};
use tonic_health::server::HealthReporter;

/// Builds the gRPC Health Checking Protocol service pair.
/// The HealthReporter handle can update statuses at runtime.
/// The HealthServer is added to the tonic Router in main.
pub async fn health_service() -> (HealthReporter, HealthServer<impl Health>) {
    let (reporter, server) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", ServingStatus::Serving)
        .await;
    (reporter, server)
}

/// Build an axum router with all gRPC and BFF HTTP services.
///
/// gRPC services are merged via `tonic::service::Routes::into_axum_router()`.
/// BFF HTTP routes are merged on top so both share a single listener.
pub async fn router<S: crate::store::Store>(
    db: Arc<S>,
    cfg: Arc<crate::config::Config>,
    http_client: reqwest::Client,
) -> axum::Router {
    let (_health_reporter, health_svc) = health_service().await;
    let admin_svc = causes_proto::admin_service_server::AdminServiceServer::new(
        crate::admin_service::AdminHandler::new(db.clone()),
    );
    let auth_svc = causes_proto::auth_service_server::AuthServiceServer::new(
        crate::auth::AuthHandler::new(db, cfg.clone(), http_client),
    );

    let grpc_router = tonic::service::Routes::new(health_svc)
        .add_service(admin_svc)
        .add_service(auth_svc)
        .into_axum_router();

    let bff_router = crate::bff::router(cfg);

    grpc_router.merge(bff_router)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that the gRPC health service can be constructed.
    #[tokio::test]
    async fn health_service_can_be_constructed() {
        let (_reporter, _health_svc) = health_service().await;
    }
}
