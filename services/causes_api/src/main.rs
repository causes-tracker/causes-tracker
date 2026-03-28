use anyhow::Context;
use clap::Parser;
use tracing::{Instrument as _, info, info_span};

mod config;
mod grpc;
mod telemetry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (no error if absent).
    dotenvy::dotenv().ok();

    let cfg = config::Config::parse();

    let _otel = telemetry::init(
        "causes-api",
        cfg.honeycomb_api_key.as_deref(),
        &cfg.honeycomb_endpoint,
    );

    startup(&cfg).instrument(info_span!("startup")).await
}

async fn startup(cfg: &config::Config) -> anyhow::Result<()> {
    info!("connecting to database");
    let pool = api_db::DbPool::connect(&cfg.database_url)
        .await
        .context("connecting to database")?;

    pool.migrate().await.context("running migrations")?;

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
