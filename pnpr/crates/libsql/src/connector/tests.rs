use super::{LibsqlConnector, dial};
use crate::testing::{EXECUTE_RESPONSE, serve_plain_http};
use http_0_2::Uri;
use pnpr_config::LibsqlSettings;
use pnpr_error::{RegistryError, Result};
use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower_0_4::{ServiceExt, service_fn};

/// Open the database `settings` name and run one statement on it.
async fn query_once(settings: &LibsqlSettings) -> Result<()> {
    let db = crate::open(settings, crate::connector(settings, "backend.libsql")?).await?;
    db.connect()?
        .execute("SELECT 1", ())
        .await?;
    Ok(())
}

fn settings(url: String) -> LibsqlSettings {
    LibsqlSettings {
        url,
        auth_token: None,
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    }
}

#[test]
fn reads_the_http_scheme_case_insensitively() {
    assert!(LibsqlConnector::for_url("HTTP://127.0.0.1:8080", false).unwrap().tls.is_none());
}

#[tokio::test]
async fn refuses_an_auth_token_over_plain_http_to_a_remote_host() {
    let settings = LibsqlSettings {
        auth_token: Some("secret".to_string()),
        ..settings("http://db.example.com:8080".to_string())
    };
    let error = query_once(&settings).await.unwrap_err();
    assert!(
        matches!(&error, RegistryError::InvalidConfig { reason } if reason.contains("backend.libsql.authToken")),
        "{error:?}",
    );
}

#[tokio::test]
async fn open_refuses_the_token_over_plain_http_whatever_the_connector() {
    let settings = LibsqlSettings {
        auth_token: Some("secret".to_string()),
        ..settings("http://db.example.com:8080".to_string())
    };
    // A connector built as if no token were sent.
    let connector = LibsqlConnector::for_url(&settings.url, false).unwrap();
    let db = crate::open(&settings, connector).await.unwrap();
    let error = db
        .connect()
        .unwrap()
        .execute("SELECT 1", ())
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("non-loopback host"), "{error:?}");
}

#[tokio::test]
async fn sends_an_auth_token_over_plain_http_to_a_loopback_host() {
    let server = serve_plain_http(EXECUTE_RESPONSE);
    let settings =
        LibsqlSettings { auth_token: Some("secret".to_string()), ..settings(server.url.clone()) };
    // Only the request that reached the server matters here.
    let _ = query_once(&settings).await;
    let head = server.requests
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(head.to_ascii_lowercase().contains("authorization: bearer secret"), "{head}");
}

#[tokio::test]
async fn refuses_to_carry_a_token_over_plain_http_off_loopback() {
    let error = LibsqlConnector::for_url("http://127.0.0.1:8080", true)
        .unwrap()
        .oneshot("http://db.example.com:8080/v2/pipeline".parse().unwrap())
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("non-loopback host"), "{error}");
}

#[tokio::test]
async fn refuses_an_uppercase_https_uri_on_a_connector_for_http() {
    // Built from parts, so the scheme keeps its case, unlike a parsed URI.
    let uri = Uri::builder()
        .scheme("HTTPS")
        .authority("127.0.0.1:8080")
        .path_and_query("/v2/pipeline")
        .build()
        .unwrap();
    assert_eq!(uri.scheme_str(), Some("HTTPS"));
    let error = LibsqlConnector::for_url("http://127.0.0.1:8080", true)
        .unwrap()
        .oneshot(uri)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("cannot be dialed over TLS"), "{error}");
}

#[tokio::test]
async fn reports_an_unreachable_database_as_an_error() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}");
    assert!(query_once(&settings(url)).await.is_err());
}

/// Dial through a stub that fails with `kind` `failures` times, then
/// connects, and return the result with the number of attempts.
async fn dial_failing(
    connector: &LibsqlConnector,
    kind: std::io::ErrorKind,
    failures: usize,
) -> (bool, usize) {
    let attempts = Arc::new(AtomicUsize::new(0));
    let dialer = service_fn(|_: Uri| {
        let attempt = attempts.fetch_add(1, Ordering::Relaxed);
        async move { if attempt < failures { Err(std::io::Error::from(kind)) } else { Ok(()) } }
    });
    let uri = "http://127.0.0.1:8080".parse().unwrap();
    let result = dial(dialer, &uri, &connector.retries_timed_out_dials).await;
    (result.is_ok(), attempts.load(Ordering::Relaxed))
}

#[tokio::test]
async fn retries_a_timed_out_dial_while_the_guard_is_alive() {
    let connector = LibsqlConnector::for_url("http://127.0.0.1:8080", false).unwrap();
    let _guard = connector.retry_timed_out_dials();
    let outcome = dial_failing(&connector, std::io::ErrorKind::TimedOut, 2).await;
    assert_eq!(outcome, (true, 3));
}

#[tokio::test]
async fn fails_a_timed_out_dial_once_the_guard_drops() {
    let connector = LibsqlConnector::for_url("http://127.0.0.1:8080", false).unwrap();
    drop(connector.retry_timed_out_dials());
    let outcome = dial_failing(&connector, std::io::ErrorKind::TimedOut, 1).await;
    assert_eq!(outcome, (false, 1));
}

#[tokio::test]
async fn fails_a_refused_dial_without_retrying() {
    let connector = LibsqlConnector::for_url("http://127.0.0.1:8080", false).unwrap();
    let _guard = connector.retry_timed_out_dials();
    let outcome = dial_failing(&connector, std::io::ErrorKind::ConnectionRefused, 1).await;
    assert_eq!(outcome, (false, 1));
}
