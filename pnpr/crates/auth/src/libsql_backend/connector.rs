//! The HTTP connector libsql dials the database with.
//!
//! libsql's own TLS connector would add a second rustls release to the
//! build, so pnpr supplies one built on the workspace's rustls.

use http_0_2::Uri;
use hyper_0_14::client::{
    HttpConnector,
    connect::{Connected, Connection},
};
use libsql::Error as LibsqlError;
use pnpm_network::is_url_secure_for_credentials;
use rustls::{ClientConfig, pki_types::ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
};
use tokio_rustls::{TlsConnector, client::TlsStream};
use tower_0_4::Service;

/// Dials `https://` URLs over TLS and `http://` URLs in plain text.
#[derive(Clone)]
pub(super) struct LibsqlConnector {
    tcp: HttpConnector,
    /// `None` for an `http://` database, which then needs no trust store.
    tls: Option<TlsConnector>,
    /// Whether requests carry an auth token, which then must not cross a
    /// cleartext link to a host other than loopback.
    carries_token: bool,
}

impl LibsqlConnector {
    /// The connector for the database at `url`. libsql dials every URL
    /// other than `http://`, such as `libsql://`, over HTTPS.
    pub(super) fn for_url(url: &str, carries_token: bool) -> Result<Self, LibsqlError> {
        let connector =
            if is_plain_http(url) { Self::with_tls(None) } else { Self::with_platform_roots()? };
        Ok(Self { carries_token, ..connector })
    }

    /// A connector that verifies servers against the platform trust store.
    pub(super) fn with_platform_roots() -> Result<Self, LibsqlError> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(BuilderVerifierExt::with_platform_verifier)
            .map_err(|error| LibsqlError::InvalidTlsConfiguration(io::Error::other(error)))?
            .with_no_client_auth();
        Ok(Self::new(config))
    }

    pub(super) fn new(config: ClientConfig) -> Self {
        Self::with_tls(Some(TlsConnector::from(Arc::new(config))))
    }

    fn with_tls(tls: Option<TlsConnector>) -> Self {
        let mut tcp = HttpConnector::new();
        tcp.enforce_http(false);
        tcp.set_nodelay(true);
        Self { tcp, tls, carries_token: false }
    }

    /// Why `uri` must not be dialed, if it must not.
    fn refusal(&self, uri: &Uri) -> Option<&'static str> {
        let https = uri
            .scheme_str()
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"));
        match (https, self.tls.is_some()) {
            (true, false) => Some("an http:// database URL cannot be dialed over TLS"),
            // A database can name a `base_url` for later requests, and libsql
            // sends the auth token with each one.
            (false, true) => Some("refusing plain HTTP to a database reached over HTTPS"),
            (false, false)
                if self.carries_token && !is_url_secure_for_credentials(&uri.to_string()) =>
            {
                Some("refusing to send the auth token over plain HTTP to a non-loopback host")
            }
            _ => None,
        }
    }
}

/// Whether libsql dials `url` without TLS. The scheme is case-insensitive.
pub(super) fn is_plain_http(url: &str) -> bool {
    url.get(.."http://".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl Service<Uri> for LibsqlConnector {
    type Response = MaybeTlsStream;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<MaybeTlsStream, BoxError>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.tcp.poll_ready(context).map_err(Into::into)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        if let Some(reason) = self.refusal(&uri) {
            return Box::pin(async move { Err(reason.into()) });
        }
        let tls = self.tls.clone();
        let tcp = self.tcp.call(uri.clone());
        Box::pin(async move {
            let stream = tcp.await?;
            let Some(tls) = tls else {
                return Ok(MaybeTlsStream::Plain(stream));
            };
            let host = uri.host().ok_or("database URL has no host")?;
            // `Uri::host` keeps the brackets of an IPv6 literal.
            let host = host.trim_start_matches('[').trim_end_matches(']');
            let server_name = ServerName::try_from(host.to_owned())?;
            let stream = tls.connect(server_name, stream).await?;
            Ok(MaybeTlsStream::Tls(Box::new(stream)))
        })
    }
}

pub(super) enum MaybeTlsStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl Connection for MaybeTlsStream {
    fn connected(&self) -> Connected {
        match self {
            Self::Plain(stream) => stream.connected(),
            Self::Tls(stream) => stream.get_ref().0.connected(),
        }
    }
}

impl AsyncRead for MaybeTlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(context, buf),
            Self::Tls(stream) => Pin::new(stream).poll_read(context, buf),
        }
    }
}

impl AsyncWrite for MaybeTlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(context, buf),
            Self::Tls(stream) => Pin::new(stream).poll_write(context, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(context),
            Self::Tls(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Tls(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

#[cfg(test)]
mod tests;
