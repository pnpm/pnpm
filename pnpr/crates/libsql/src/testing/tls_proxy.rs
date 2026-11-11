use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_rustls::TlsAcceptor;

/// Terminate TLS on loopback with `(cert_pem, key_pem)` and forward each
/// connection to `upstream`.
pub async fn serve_tls_proxy(
    upstream: SocketAddr,
    cert_pem: &[u8],
    key_pem: &[u8],
) -> (SocketAddr, JoinHandle<()>) {
    let acceptor = TlsAcceptor::from(server_config(cert_pem, key_pem));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind TLS proxy");
    let address = listener.local_addr().expect("TLS proxy address");
    let handle = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(forward_connection(acceptor, stream, upstream));
        }
    });
    (address, handle)
}

async fn forward_connection(acceptor: TlsAcceptor, stream: TcpStream, upstream: SocketAddr) {
    let Ok(mut client) = acceptor.accept(stream).await else { return };
    let Ok(mut backend) = TcpStream::connect(upstream).await else { return };
    let _ = copy_bidirectional(&mut client, &mut backend).await;
}

fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Arc<ServerConfig> {
    let cert = CertificateDer::from_pem_slice(cert_pem).expect("parse server certificate");
    let key = PrivateKeyDer::from_pem_slice(key_pem).expect("parse server key");
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    Arc::new(
        ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("supported TLS protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("configure TLS server"),
    )
}
