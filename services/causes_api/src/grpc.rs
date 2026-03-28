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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that the gRPC health service can be constructed.
    #[tokio::test]
    async fn health_service_can_be_constructed() {
        let (_reporter, _health_svc) = health_service().await;
    }
}
