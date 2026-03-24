use tonic_health::server::HealthReporter;
use tonic_health::ServingStatus;

/// Builds the gRPC Health Checking Protocol service pair.
/// The HealthReporter handle can update statuses at runtime.
/// The HealthServer is added to the tonic Router in main.
pub async fn health_service()
-> (HealthReporter, tonic_health::server::HealthServer<tonic_health::server::HealthService>)
{
    let (mut reporter, server) = tonic_health::server::health_reporter();
    reporter.set_service_status("", ServingStatus::Serving).await;
    (reporter, server)
}
