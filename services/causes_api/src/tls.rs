use std::io;
use std::net::SocketAddr;
use std::pin::Pin;

use anyhow::Context as _;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::info;

/// Start the server with automatic TLS on port 443 using Let's Encrypt.
///
/// In TLS mode we also start a loopback-only plain gRPC listener on
/// 127.0.0.1:50051 so that BFF handlers can reach the gRPC services
/// without going through TLS.
pub async fn serve_with_acme<S: crate::store::Store>(
    cfg: std::sync::Arc<crate::config::Config>,
    db: std::sync::Arc<S>,
    http_client: reqwest::Client,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let domain = cfg.tls_domain.clone().expect("tls_domain must be set");
    let cache_dir = cfg.tls_cert_cache_dir.clone();

    let mut acme = AcmeConfig::new([domain.clone()])
        .cache(DirCache::new(cache_dir))
        .directory_lets_encrypt(true);

    if let Some(email) = cfg.tls_acme_email.as_deref() {
        acme = acme.contact_push(format!("mailto:{email}"));
    }

    serve_with_config(acme, "[::]:443", cfg, db, http_client, shutdown).await
}

/// Common worker: binds the listener, wires up services, and serves.
async fn serve_with_config<S, EC, EA>(
    acme: AcmeConfig<EC, EA>,
    bind_addr: &str,
    cfg: std::sync::Arc<crate::config::Config>,
    db: std::sync::Arc<S>,
    http_client: reqwest::Client,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()>
where
    S: crate::store::Store,
    EC: 'static + Send + Sync + std::fmt::Debug,
    EA: 'static + Send + Sync + std::fmt::Debug,
{
    let listener = TcpListener::bind(bind_addr)
        .await
        .context("binding TLS listener")?;

    let local_addr = listener.local_addr().context("getting local address")?;
    info!(%local_addr, "TLS server listening");

    let tcp_stream = TcpListenerStream::new(listener);

    // h2 ALPN for gRPC; rustls-acme adds acme-tls/1 internally for challenges.
    let tls_incoming = acme.tokio_incoming(tcp_stream, vec![b"h2".to_vec()]);

    // Start a loopback-only plain gRPC listener for BFF→gRPC internal calls.
    // Bind before building the router so we can tell the BFF the actual port.
    let loopback = TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding loopback gRPC listener")?;
    let loopback_addr = loopback.local_addr().context("loopback local addr")?;
    info!(%loopback_addr, "loopback gRPC listener (plain, internal)");

    let grpc_url = format!("http://{loopback_addr}");
    let router = crate::grpc::router(db, cfg, http_client, grpc_url).await;
    let loopback_router = router.clone();
    tokio::spawn(async move {
        axum::serve(loopback, loopback_router).await.ok();
    });

    let acme_listener = AcmeListener {
        stream: Box::pin(tls_incoming),
        local_addr,
    };

    axum::serve(acme_listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .context("TLS server error")?;

    Ok(())
}

// ── AcmeListener: adapts a rustls-acme TLS stream to axum's Listener trait ──

/// Wraps a rustls-acme TLS incoming stream so it implements `axum::serve::Listener`.
struct AcmeListener<IO> {
    stream: Pin<Box<dyn tokio_stream::Stream<Item = Result<IO, io::Error>> + Send>>,
    local_addr: SocketAddr,
}

impl<IO> axum::serve::Listener for AcmeListener<IO>
where
    IO: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Io = IO;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        use tokio_stream::StreamExt as _;
        loop {
            match self.stream.next().await {
                Some(Ok(io)) => return (io, self.local_addr),
                Some(Err(e)) => {
                    tracing::warn!("TLS accept error: {e}");
                    continue;
                }
                None => {
                    // Stream ended; block forever (server will shut down
                    // via the graceful shutdown signal).
                    std::future::pending::<()>().await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> crate::config::Config {
        use clap::Parser;
        crate::config::Config::parse_from([
            "causes_api",
            "--database-url=postgresql://unused",
            "--tls-domain=test.example.com",
            "--tls-cert-cache-dir=/tmp/causes-tls-test",
        ])
    }

    fn test_store() -> crate::store::MockStore {
        let mut db = crate::store::MockStore::new();
        db.expect_migrate().returning(|| Ok(()));
        db.expect_user_count().returning(|| Ok(1));
        db
    }

    #[tokio::test]
    async fn serve_with_config_starts_and_shuts_down() {
        use rustls_acme::caches::DirCache;

        let cache_dir =
            std::env::temp_dir().join(format!("causes-tls-test-{}", std::process::id()));
        let cache_dir_str = cache_dir.to_str().unwrap().to_owned();

        // Point ACME at a directory URL that will never respond — we're
        // testing that the server binds, registers services, and shuts
        // down cleanly, not that ACME completes.
        let acme = AcmeConfig::new(["test.example.com".to_owned()])
            .cache(DirCache::new(cache_dir_str))
            .directory("https://127.0.0.1:1/.well-known/acme");

        let cfg = std::sync::Arc::new(test_config());
        let db = std::sync::Arc::new(test_store());

        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        let handle = tokio::spawn(serve_with_config(
            acme,
            "127.0.0.1:0",
            cfg,
            db,
            reqwest::Client::new(),
            async {
                rx.await.ok();
            },
        ));

        // Give the server a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Signal shutdown.
        tx.send(()).expect("receiver dropped");
        handle.await.unwrap().expect("serve_with_config failed");

        std::fs::remove_dir_all(&cache_dir).ok();
    }

    #[tokio::test]
    async fn acme_listener_yields_connections() {
        let (client_io, server_io) = tokio::io::duplex(64);
        let stream = tokio_stream::once(Ok::<_, io::Error>(server_io));
        let addr: SocketAddr = "127.0.0.1:4433".parse().unwrap();

        let mut listener = AcmeListener {
            stream: Box::pin(stream),
            local_addr: addr,
        };

        let (io, reported_addr) = axum::serve::Listener::accept(&mut listener).await;
        assert_eq!(reported_addr, addr);

        // Verify the IO is functional by writing through the duplex.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut client_io = client_io;
        client_io.write_all(b"hello").await.unwrap();
        client_io.shutdown().await.unwrap();

        let mut io = io;
        let mut buf = vec![0u8; 5];
        io.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn acme_listener_skips_errors() {
        let err = io::Error::new(io::ErrorKind::Other, "handshake failed");
        let (_, good_io) = tokio::io::duplex(64);
        let stream = tokio_stream::iter(vec![Err(err), Ok(good_io)]);
        let addr: SocketAddr = "127.0.0.1:4433".parse().unwrap();

        let mut listener = AcmeListener {
            stream: Box::pin(stream),
            local_addr: addr,
        };

        // Should skip the error and return the good connection.
        let (_io, reported_addr) = axum::serve::Listener::accept(&mut listener).await;
        assert_eq!(reported_addr, addr);
    }

    #[test]
    fn acme_listener_local_addr() {
        let stream = tokio_stream::empty::<Result<tokio::io::DuplexStream, io::Error>>();
        let addr: SocketAddr = "127.0.0.1:4433".parse().unwrap();
        let listener = AcmeListener {
            stream: Box::pin(stream),
            local_addr: addr,
        };
        assert_eq!(axum::serve::Listener::local_addr(&listener).unwrap(), addr);
    }
}
