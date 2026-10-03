use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// An HTTPS server on loopback that answers every request with a fixed JSON
/// body. Its certificate is issued for `127.0.0.1` by the CA at
/// [`Self::ca_path`], so a client trusts it only when given that CA.
pub struct TrustedTlsServer {
    pub url: String,
}

impl TrustedTlsServer {
    #[must_use]
    pub fn start(body: &str) -> Self {
        let cert = CertificateDer::from_pem_slice(include_bytes!("fixtures/tls/server.crt"))
            .expect("parse server certificate");
        let key = PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/tls/server.key"))
            .expect("parse server key");
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .expect("configure trusted TLS server"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS server");
        let url = format!("https://{}", listener.local_addr().expect("TLS server address"));
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len(),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                let Ok(connection) = ServerConnection::new(Arc::clone(&config)) else {
                    continue;
                };
                let mut tls = BufReader::new(StreamOwned::new(connection, stream));
                let mut line = String::new();
                while tls
                    .read_line(&mut line)
                    .is_ok_and(|read| read > 0)
                    && line != "\r\n"
                {
                    line.clear();
                }
                let tls = tls.get_mut();
                let _ = tls.write_all(response.as_bytes());
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        Self { url }
    }

    /// The PEM file of the CA that issued the server certificate.
    #[must_use]
    pub fn ca_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/tls/ca.pem")
    }
}
