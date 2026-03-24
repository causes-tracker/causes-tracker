use std::future::Future;

use anyhow::Context;
use clap::Parser;
use tracing::{Instrument as _, info, info_span};

mod bootstrap;
mod config;
mod google;
mod grpc;
mod store;
mod telemetry;

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
    db: impl store::Store,
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
    db: impl store::Store,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    db.migrate().await.context("running migrations")?;

    info!("database ready");

    let http_client = reqwest::Client::new();
    bootstrap::run(&db, cfg, &http_client)
        .await
        .context("bootstrap failed")?;

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
    use super::{config, main_inner, store};

    struct FakeDb;

    impl store::Store for FakeDb {
        async fn migrate(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn user_count(&self) -> anyhow::Result<i64> {
            // Return > 0 so bootstrap short-circuits.
            Ok(1)
        }

        async fn create_admin(
            &self,
            _display_name: &api_db::DisplayName,
            _email: &api_db::Email,
            _auth_provider: &api_db::AuthProvider,
            _subject: &api_db::Subject,
        ) -> anyhow::Result<api_db::UserId> {
            unreachable!("bootstrap short-circuits when user_count > 0")
        }
    }

    /// Exercises main_inner through startup and then shuts down cleanly.
    #[tokio::test]
    async fn startup_migrates_and_binds() {
        let cfg = config::Config {
            database_url: "unused".to_string(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
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
