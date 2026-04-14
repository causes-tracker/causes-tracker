use std::future::Future;

use anyhow::Context;
use clap::Parser;
use tracing::{Instrument as _, info, info_span};

mod admin_service;
mod auth;
mod bff;
mod bootstrap;
mod config;
mod google;
mod grpc;
mod interceptor;
mod project;
mod store;
mod telemetry;
mod tls;

/// Production entry point for the Causes API service.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (no error if absent).
    dotenvy::dotenv().ok();

    let cfg = config::Config::parse();

    info!("connecting to database");
    let pool = api_db::DbPool::from_config(
        cfg.db_host.as_deref(),
        cfg.db_user.as_deref(),
        cfg.db_port,
        cfg.database_url.as_deref(),
    )
    .await
    .context("connecting to database")?;
    let _refresh_task = pool.start_background_refresh();

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

    startup(cfg, db, shutdown)
        .instrument(info_span!("startup"))
        .await
}

async fn startup(
    cfg: config::Config,
    db: impl store::Store,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    db.migrate().await.context("running migrations")?;

    info!("database ready");

    let http_client = reqwest::Client::new();
    bootstrap::run(&db, &cfg, &http_client)
        .await
        .context("bootstrap failed")?;

    let db = std::sync::Arc::new(db);

    // Background GC: clean up expired sessions and abandoned pending logins.
    let gc_db = db.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            if let Err(e) = gc_db.gc_expired_sessions().await {
                tracing::warn!("session GC failed: {e}");
            }
            if let Err(e) = gc_db
                .gc_pending_logins(std::time::Duration::from_secs(3600))
                .await
            {
                tracing::warn!("pending login GC failed: {e}");
            }
        }
    });

    if cfg.tls_domain.is_some() {
        tls::serve_with_acme(std::sync::Arc::new(cfg), db, http_client, shutdown)
            .await
            .context("TLS gRPC server error")?;
    } else {
        let addr: std::net::SocketAddr = cfg.bind_addr.parse().context("parsing BIND_ADDR")?;
        let grpc_url = format!("http://{}", cfg.bind_addr);
        let router = grpc::router(db, std::sync::Arc::new(cfg), http_client, grpc_url).await;

        info!(%addr, "server listening (plain HTTP/2)");

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .context("binding listener")?;
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await
            .context("server error")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{config, main_inner};
    use crate::store::MockStore;

    /// Exercises main_inner through startup and then shuts down cleanly.
    #[tokio::test]
    async fn startup_migrates_and_binds() {
        let mut db = MockStore::new();
        db.expect_migrate().returning(|| Ok(()));
        // Return > 0 so bootstrap short-circuits.
        db.expect_user_count().returning(|| Ok(1));

        let cfg = config::Config {
            database_url: Some("unused".to_string()),
            db_host: None,
            db_user: None,
            db_port: 5432,
            google_client_id: String::new(),
            google_client_secret: String::new(),
            honeycomb_api_key: None,
            honeycomb_endpoint: "https://api.honeycomb.io:443".to_string(),
            bind_addr: "127.0.0.1:0".to_string(),
            tls_domain: None,
            tls_acme_email: None,
            tls_cert_cache_dir: "/tmp/certs".to_string(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle: tokio::task::JoinHandle<anyhow::Result<()>> =
            tokio::spawn(main_inner(cfg, db, async {
                rx.await.ok();
            }));
        // Give the server a moment to bind, then signal shutdown.
        tokio::task::yield_now().await;
        tx.send(()).expect("receiver dropped");
        handle.await.unwrap().expect("main_inner returned an error");
    }
}
