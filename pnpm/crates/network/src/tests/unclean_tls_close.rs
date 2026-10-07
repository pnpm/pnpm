use super::{NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig};
use crate::read_self_delimiting_text;
use flate2::{Compression, write::GzEncoder};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    sync::Arc,
    thread::JoinHandle,
};

const DOCUMENT: &str = r#"{"name":"foo","dist-tags":{"latest":"1.0.0"}}"#;

/// Serve one HTTPS response, then close the TCP connection without a TLS
/// `close_notify` alert.
fn serve_then_close_uncleanly(
    extra_headers: &'static str,
    body: Vec<u8>,
) -> (SocketAddr, JoinHandle<()>) {
    let cert = CertificateDer::from_pem_slice(include_bytes!(
        "../../tests/fixtures/test-client-pkcs1.crt"
    ))
    .expect("parse server certificate");
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("../../tests/fixtures/test-client-pkcs1.key"))
            .expect("parse server key");
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("configure TLS server");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS server");
    let address = listener.local_addr().expect("TLS server address");
    let server = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().expect("accept TLS connection");
        let connection = ServerConnection::new(Arc::new(config)).expect("create TLS connection");
        let mut stream = StreamOwned::new(connection, tcp);
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            let read = stream.read(&mut buffer).expect("read request");
            assert_ne!(read, 0, "client closed before sending the request");
            request.extend_from_slice(&buffer[..read]);
        }
        let head = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{extra_headers}\r\n");
        stream.write_all(head.as_bytes()).expect("write response head");
        stream.write_all(&body).expect("write response body");
        stream.flush().expect("flush response");
        // Dropping the stream closes the socket without `send_close_notify`.
    });
    (address, server)
}

fn client_trusting_any_certificate() -> ThrottledClient {
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig { strict_ssl: Some(false), ..TlsConfig::default() },
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("build client")
}

async fn fetch_text(address: SocketAddr) -> Result<String, reqwest::Error> {
    let client = client_trusting_any_certificate();
    let response = client
        .acquire()
        .await
        .get(format!("https://{address}/foo"))
        .send()
        .await
        .expect("receive response head");
    read_self_delimiting_text(response).await
}

#[tokio::test]
async fn close_delimited_body_ends_at_unclean_tls_close() {
    let (address, server) = serve_then_close_uncleanly("", DOCUMENT.as_bytes().to_vec());
    let body = fetch_text(address).await.expect("read body");
    server.join().expect("TLS server thread");
    assert_eq!(body, DOCUMENT);
}

#[tokio::test]
async fn gzipped_close_delimited_body_ends_at_unclean_tls_close() {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(DOCUMENT.as_bytes()).expect("gzip document");
    let gzipped = encoder.finish().expect("finish gzip");
    let (address, server) = serve_then_close_uncleanly("Content-Encoding: gzip\r\n", gzipped);
    let body = fetch_text(address).await.expect("read body");
    server.join().expect("TLS server thread");
    assert_eq!(body, DOCUMENT);
}

#[tokio::test]
async fn cut_short_content_length_body_fails_at_unclean_tls_close() {
    let (address, server) =
        serve_then_close_uncleanly("Content-Length: 100\r\n", DOCUMENT.as_bytes().to_vec());
    let error = fetch_text(address).await.expect_err("a cut-short body fails");
    server.join().expect("TLS server thread");
    eprintln!("body error: {error:?}");
    assert!(error.is_body() || error.is_decode(), "expected a body error: {error:?}");
}

#[tokio::test]
async fn cut_short_chunked_body_fails_at_unclean_tls_close() {
    let (address, server) = serve_then_close_uncleanly(
        "Transfer-Encoding: chunked\r\n",
        format!("{:x}\r\n{DOCUMENT}\r\n", DOCUMENT.len()).into_bytes(),
    );
    let error = fetch_text(address).await.expect_err("a body without its last chunk fails");
    server.join().expect("TLS server thread");
    eprintln!("body error: {error:?}");
    assert!(error.is_body() || error.is_decode(), "expected a body error: {error:?}");
}
