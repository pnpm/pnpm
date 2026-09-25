use rustls::{
    ServerConfig, ServerConnection,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// An HTTPS server on loopback whose self-signed certificate no client
/// trusts. It counts the TLS connections it accepts, so a test can tell
/// how many times a client tried before giving up.
pub struct UntrustedTlsServer {
    pub url: String,
    connections: Arc<AtomicUsize>,
}

impl UntrustedTlsServer {
    #[must_use]
    pub fn start() -> Self {
        let cert = CertificateDer::from_pem_slice(include_bytes!(
            "../../network/tests/fixtures/test-client-pkcs1.crt"
        ))
        .expect("parse server certificate");
        let key = PrivateKeyDer::from_pem_slice(include_bytes!(
            "../../network/tests/fixtures/test-client-pkcs1.key"
        ))
        .expect("parse server key");
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .expect("configure untrusted TLS server"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS server");
        let url = format!("https://{}", listener.local_addr().expect("TLS server address"));
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&connections);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                let Ok(mut connection) = ServerConnection::new(Arc::clone(&config)) else {
                    continue;
                };
                let _ = connection.complete_io(&mut stream);
            }
        });
        Self { url, connections }
    }

    #[must_use]
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}
