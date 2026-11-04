//! The libSQL connector against servers behind TLS and the platform trust
//! store. These run here rather than in `pnpr-libsql` because the TLS test
//! server comes with `pnpm-testing-utils`, and this crate's test binary links
//! the second `SQLite` that dependency brings.

use libsql::Builder;
use pnpm_testing_utils::{env_guard::EnvGuard, trusted_tls_server::TrustedTlsServer};
use pnpr_config::LibsqlSettings;
use pnpr_error::Result;
use pnpr_libsql::{
    LibsqlConnector,
    testing::{EXECUTE_RESPONSE, serve_plain_http},
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use std::sync::Arc;
use tower_0_4::ServiceExt;

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

/// Open the database at `url` and run one statement on it.
async fn query_once(url: &str) -> Result<()> {
    let settings = LibsqlSettings {
        url: url.to_string(),
        auth_token: None,
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    };
    let db =
        pnpr_libsql::open(&settings, pnpr_libsql::connector(&settings, "backend.libsql")?).await?;
    db.connect()?
        .execute("SELECT 1", ())
        .await?;
    Ok(())
}

#[tokio::test]
async fn queries_a_remote_database_over_https() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
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
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    // Keeps the empty trust store of the test below out of this one.
    let _env = EnvGuard::snapshot(["SSL_CERT_FILE", "SSL_CERT_DIR"]);
    let server = TrustedTlsServer::start(EXECUTE_RESPONSE);
    let error = query_once(&server.url).await.unwrap_err();
    assert!(format!("{error:?}").contains("invalid peer certificate"), "{error:?}");
}

#[tokio::test]
async fn refuses_plain_http_on_a_connector_for_https() {
    let server = serve_plain_http(EXECUTE_RESPONSE);
    let error = connector_trusting_test_ca()
        .oneshot(server.url.parse().unwrap())
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("refusing plain HTTP"), "{error}");
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

    let server = serve_plain_http(EXECUTE_RESPONSE);
    let connector = LibsqlConnector::for_url(&server.url, false).unwrap();
    let db = Builder::new_remote(server.url, String::new())
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
