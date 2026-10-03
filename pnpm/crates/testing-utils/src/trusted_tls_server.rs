use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// An HTTPS server on loopback that answers every request with a fixed JSON
/// body. Its certificate is issued for `127.0.0.1` by the CA at
/// [`Self::ca_path`], so a client trusts it only when given that CA.
///
/// The fixtures under `src/fixtures/tls/` were generated with:
///
/// ```text
/// openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
///     -days 36500 -subj '/CN=pacquet-test-server-ca' -keyout ca.key -out ca.pem \
///     -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign'
/// openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
///     -subj '/CN=localhost' -keyout server.key -out server.csr
/// openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
///     -days 36500 -out server.crt -extfile <(printf '%s\n' \
///     'subjectAltName=DNS:localhost,IP:127.0.0.1' 'basicConstraints=critical,CA:FALSE' \
///     'extendedKeyUsage=serverAuth' 'keyUsage=critical,digitalSignature')
/// ```
///
/// The CA key is discarded.
pub struct TrustedTlsServer {
    pub url: String,
}

impl TrustedTlsServer {
    #[must_use]
    pub fn start(body: &str) -> Self {
        let config = server_config(
            include_bytes!("fixtures/tls/server.crt"),
            include_bytes!("fixtures/tls/server.key"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS server");
        let url = format!("https://{}", listener.local_addr().expect("TLS server address"));
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len(),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream, &config, &response);
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

pub(crate) fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Arc<ServerConfig> {
    let cert = CertificateDer::from_pem_slice(cert_pem).expect("parse server certificate");
    let key = PrivateKeyDer::from_pem_slice(key_pem).expect("parse server key");
    Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("configure TLS server"),
    )
}

/// Read one request's head off `stream` and answer it with `response`.
fn serve(stream: TcpStream, config: &Arc<ServerConfig>, response: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let Ok(connection) = ServerConnection::new(Arc::clone(config)) else {
        return;
    };
    let mut tls = BufReader::new(StreamOwned::new(connection, stream));
    let mut line = String::new();
    loop {
        line.clear();
        match tls.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) if line == "\r\n" => break,
            Ok(_) => {}
        }
    }
    let tls = tls.get_mut();
    let _ = tls.write_all(response.as_bytes());
    tls.conn.send_close_notify();
    let _ = tls.flush();
}
