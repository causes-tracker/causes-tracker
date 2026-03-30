use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Context as _;
use rustls_acme::AcmeConfig;
use rustls_acme::caches::DirCache;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::info;

/// Start the gRPC server with automatic TLS on port 443.
///
/// Uses `rustls-acme` for ACME TLS-ALPN-01 certificate issuance and renewal.
/// gRPC (ALPN `h2`) and ACME challenges (ALPN `acme-tls/1`) share the same
/// port — `rustls-acme` handles the challenge connections internally and
/// passes normal traffic through.
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

    let listener = TcpListener::bind("[::]:443")
        .await
        .context("binding port 443")?;

    info!(%domain, "TLS server listening on port 443");

    let tcp_stream = TcpListenerStream::new(listener);

    // h2 ALPN for gRPC; rustls-acme adds acme-tls/1 internally for challenges.
    let tls_incoming = acme.tokio_incoming(tcp_stream, vec![b"h2".to_vec()]);

    let (_health_reporter, health_svc) = crate::grpc::health_service().await;
    let auth_svc = causes_proto::auth_service_server::AuthServiceServer::new(
        crate::auth::AuthHandler::new(db, cfg, http_client),
    );

    // rustls-acme's tokio_incoming already yields Compat<TlsStream<...>> which
    // implements tokio::AsyncRead/AsyncWrite.  We just need the AcmeTlsStream
    // wrapper to add tonic's Connected trait.
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
