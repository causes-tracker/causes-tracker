use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Context as _;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::info;

/// Start the gRPC server with automatic TLS on port 443 using Let's Encrypt.
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

    let (_health_reporter, health_svc) = crate::grpc::health_service().await;
    let auth_svc = causes_proto::auth_service_server::AuthServiceServer::new(
        crate::auth::AuthHandler::new(db, cfg, http_client),
    );

    let wrapped = tokio_stream::StreamExt::map(tls_incoming, |result| result.map(AcmeTlsStream));
    tokio::pin!(wrapped);

    tonic::transport::Server::builder()
        .add_service(health_svc)
        .add_service(auth_svc)
        .serve_with_incoming_shutdown(wrapped, shutdown)
        .await
        .context("TLS gRPC server error")?;

    Ok(())
}

// ── Newtype wrapper for tonic's Connected trait ───────────────────────────

/// Wraps the TLS stream from rustls-acme so it implements `tonic::transport::server::Connected`.
struct AcmeTlsStream<T>(T);

impl<T: AsyncRead + Unpin> AsyncRead for AcmeTlsStream<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for AcmeTlsStream<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

impl<T> tonic::transport::server::Connected for AcmeTlsStream<T> {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn acme_tls_stream_delegates_read_write() {
        let (client, server) = tokio::io::duplex(64);
        let mut wrapped = AcmeTlsStream(server);

        tokio::spawn(async move {
            let mut client = client;
            client.write_all(b"hello").await.unwrap();
            client.shutdown().await.unwrap();
        });

        let mut buf = vec![0u8; 5];
        wrapped.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn acme_tls_stream_implements_connected() {
        let (_, server) = tokio::io::duplex(64);
        let wrapped = AcmeTlsStream(server);
        let _info: () = tonic::transport::server::Connected::connect_info(&wrapped);
    }
}
