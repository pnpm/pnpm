use super::LibsqlConnector;
use crate::libsql_backend::LibsqlAuth;
use libsql::Builder;
use pnpm_testing_utils::trusted_tls_server::TrustedTlsServer;
use pnpr_config::{LibsqlSettings, MaxUsers};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use std::{net::TcpListener, sync::Arc};

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
    let server = TrustedTlsServer::start(EXECUTE_RESPONSE);
    let error = LibsqlAuth::connect(&settings(server.url.clone()), MaxUsers::Unlimited)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("certificate"), "{error:?}");
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
