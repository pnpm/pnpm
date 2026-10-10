use super::LibsqlConnector;
use crate::libsql_backend::LibsqlAuth;
use libsql::Builder;
use pnpm_testing_utils::{env_guard::EnvGuard, trusted_tls_server::TrustedTlsServer};
use pnpr_config::{LibsqlSettings, MaxUsers};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
};
use tower_0_4::ServiceExt;

/// The Hrana pipeline response to libsql's request for one statement:
/// a batch with an empty step result, the autocommit state, and the close.
const EXECUTE_RESPONSE: &str = r#"{"baton":null,"base_url":null,"results":[{"type":"ok","response":{"type":"batch","result":{"step_results":[{"cols":[],"rows":[],"affected_row_count":0,"last_insert_rowid":null}],"step_errors":[null]}}},{"type":"ok","response":{"type":"get_autocommit","is_autocommit":true}},{"type":"ok","response":{"type":"close"}}]}"#;

fn connector_trusting_test_ca() -> LibsqlConnector {
    let ca = CertificateDer::from_pem_file(TrustedTlsServer::ca_path()).unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(ca).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    LibsqlConnector::new(config)
}

fn settings(url: String) -> LibsqlSettings {
    LibsqlSettings { url, auth_token: None, replica_path: None, sync_interval_secs: None }
}

#[tokio::test]
async fn queries_a_remote_database_over_https() {
    let server = TrustedTlsServer::start(EXECUTE_RESPONSE);
    let db = Builder::new_remote(server.url.clone(), String::new())
        .connector(connector_trusting_test_ca())
        .build()
        .await
        .unwrap();
    let affected = db
        .connect()
        .unwrap()
        .execute("SELECT 1", ())
        .await
        .unwrap();
    assert_eq!(affected, 0);
}

#[tokio::test]
async fn rejects_a_server_the_platform_does_not_trust() {
    // Keeps the empty trust store of the test below out of this one.
    let _env = EnvGuard::snapshot(["SSL_CERT_FILE", "SSL_CERT_DIR"]);
    let server = TrustedTlsServer::start(EXECUTE_RESPONSE);
    let error = LibsqlAuth::connect(&settings(server.url.clone()), MaxUsers::Unlimited)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("invalid peer certificate"), "{error:?}");
}

#[tokio::test]
async fn refuses_plain_http_on_a_connector_for_https() {
    let url = serve_plain_http(EXECUTE_RESPONSE);
    let error = connector_trusting_test_ca()
        .oneshot(url.parse().unwrap())
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("refusing plain HTTP"), "{error}");
}

#[tokio::test]
async fn reports_an_unreachable_database_as_an_error() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}");
    assert!(LibsqlAuth::connect(&settings(url), MaxUsers::Unlimited).await.is_err());
}

// Only these platforms load the trust store from SSL_CERT_FILE and SSL_CERT_DIR.
#[cfg(all(unix, not(target_vendor = "apple")))]
#[tokio::test]
async fn queries_an_http_database_without_a_trust_store() {
    let env = EnvGuard::snapshot(["SSL_CERT_FILE", "SSL_CERT_DIR"]);
    let empty = tempfile::tempdir().unwrap();
    let no_certs = empty.path().join("certs.pem");
    std::fs::write(&no_certs, "").unwrap();
    env.set("SSL_CERT_FILE", &no_certs);
    env.set("SSL_CERT_DIR", empty.path());
    assert!(LibsqlConnector::with_platform_roots().is_err());

    let url = serve_plain_http(EXECUTE_RESPONSE);
    let connector = LibsqlConnector::for_url(&url).unwrap();
    let db = Builder::new_remote(url, String::new())
        .connector(connector)
        .build()
        .await
        .unwrap();
    let affected = db
        .connect()
        .unwrap()
        .execute("SELECT 1", ())
        .await
        .unwrap();
    assert_eq!(affected, 0);
}

/// Answer every request on a loopback plain-HTTP server with `body`.
fn serve_plain_http(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            answer(stream, body);
        }
    });
    url
}

fn answer(mut stream: TcpStream, body: &str) {
    let mut reader = BufReader::new(&stream);
    let content_length = read_content_length(&mut reader);
    reader
        .read_exact(&mut vec![0; content_length])
        .unwrap();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes()).unwrap();
}

/// Read a request head and return its `content-length`.
fn read_content_length(reader: &mut impl BufRead) -> usize {
    let mut content_length = 0;
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap() > 2 {
        let header = line.to_ascii_lowercase();
        if let Some(value) = header.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap();
        }
        line.clear();
    }
    content_length
}
