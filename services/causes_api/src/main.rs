use std::future::Future;

use anyhow::Context;
use clap::Parser;
use tracing::{Instrument as _, info, info_span};

mod config;
mod grpc;
mod telemetry;

/// Database operations needed by this service.
trait Db: Send + 'static {
    async fn migrate(&self) -> anyhow::Result<()>;
}

impl Db for api_db::DbPool {
    async fn migrate(&self) -> anyhow::Result<()> {
        api_db::DbPool::migrate(self).await
    }
}

/// Production entry point for the Causes API service.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (no error if absent).
    dotenvy::dotenv().ok();

    let cfg = config::Config::parse();

    info!("connecting to database");
    let pool = api_db::DbPool::connect(&cfg.database_url)
        .await
        .context("connecting to database")?;

    main_inner(cfg, pool, async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl-c handler");
    })
    .await
}

async fn main_inner(
    cfg: config::Config,
    db: impl Db,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let _otel = telemetry::init(
        "causes-api",
        cfg.honeycomb_api_key.as_deref(),
        &cfg.honeycomb_endpoint,
    );

    startup(&cfg, db, shutdown)
        .instrument(info_span!("startup"))
        .await
}

async fn startup(
    cfg: &config::Config,
    db: impl Db,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    db.migrate().await.context("running migrations")?;

    info!("database ready");

    let addr = cfg.bind_addr.parse().context("parsing BIND_ADDR")?;
    let (_health_reporter, health_svc) = grpc::health_service().await;

    info!(%addr, "gRPC server listening");

    tonic::transport::Server::builder()
        .add_service(health_svc)
        .serve_with_shutdown(addr, shutdown)
        .await
        .context("gRPC server error")?;

    drop(db);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDb;

    impl Db for FakeDb {
        async fn migrate(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Exercises main_inner through startup and then shuts down cleanly.
    #[tokio::test]
    async fn startup_migrates_and_binds() {
        let cfg = config::Config {
            database_url: "unused".to_string(),
            honeycomb_api_key: None,
            honeycomb_endpoint: "https://api.honeycomb.io:443".to_string(),
            bind_addr: "127.0.0.1:0".to_string(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(main_inner(cfg, FakeDb, async {
            rx.await.ok();
        }));
        // Give the server a moment to bind, then signal shutdown.
        tokio::task::yield_now().await;
        tx.send(()).expect("receiver dropped");
        handle.await.unwrap().expect("main_inner returned an error");
    }
}
