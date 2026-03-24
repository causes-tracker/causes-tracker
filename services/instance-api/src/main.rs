use anyhow::Context;
use clap::Parser;
use tracing::info;

mod config;
mod db;
mod grpc;
mod telemetry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (no error if absent).
    dotenvy::dotenv().ok();

    let cfg = config::Config::parse();

    let _otel = telemetry::init("instance-api", cfg.honeycomb_api_key.as_deref());

    info!("connecting to database");
    let pool = db::init(&cfg.database_url)
        .await
        .context("initialising database")?;

    info!("database ready");

    let addr = cfg.bind_addr.parse().context("parsing BIND_ADDR")?;
    let (_health_reporter, health_svc) = grpc::health_service().await;

    info!(%addr, "gRPC server listening");

    tonic::transport::Server::builder()
        .add_service(health_svc)
        .serve(addr)
        .await
        .context("gRPC server error")?;

    // Pool is kept alive until server exits.
    drop(pool);

    Ok(())
}
