use std::time::Duration;

use axum::http::StatusCode;
use tokio::net::TcpListener;

use super::RegistryError;

/// `reqwest::Error` has no public constructor, so the only way to
/// get a real `is_timeout()` error in a test is to actually time
/// out. Spin up a TCP listener that accepts and holds the socket
/// open, fire a reqwest request with a sub-second timeout against
/// it, and check the error round-trips through `status_code()`.
#[tokio::test]
async fn timeout_error_maps_to_gateway_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Keep accepted sockets alive for the duration of the test so
    // the client really hangs on read instead of seeing FIN.
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });

    let client = reqwest::Client::builder().timeout(Duration::from_millis(100)).build().unwrap();
    let url = format!("http://{addr}/");
    let err = client.get(&url).send().await.unwrap_err();
    assert!(err.is_timeout(), "expected timeout error, got {err:?}");

    let registry_err = RegistryError::Upstream { url, source: err };
    assert_eq!(registry_err.status_code(), StatusCode::GATEWAY_TIMEOUT);
}

#[test]
fn object_store_error_maps_to_bad_gateway() {
    let err = RegistryError::ObjectStore(object_store::Error::Generic {
        store: "test",
        source: "boom".into(),
    });
    assert_eq!(err.status_code(), StatusCode::BAD_GATEWAY);
}

#[test]
fn public_message_hides_server_error_details() {
    let err = RegistryError::ObjectStore(object_store::Error::Generic {
        store: "test",
        source: "internal-hostname".into(),
    });
    assert_eq!(err.status_code(), StatusCode::BAD_GATEWAY);
    assert_eq!(err.public_message(), "Bad Gateway");
    assert!(err.to_string().contains("internal-hostname"));
}

#[test]
fn log_message_keeps_non_secret_server_error_details() {
    let err =
        RegistryError::Internal { reason: "auth database COUNT(*) returned no rows".to_string() };
    assert_eq!(err.public_message(), "Internal Server Error");
    assert_eq!(err.log_message(), "Internal error: auth database COUNT(*) returned no rows");
}

#[test]
fn log_message_redacts_embedded_database_url_credentials() {
    let err = RegistryError::Internal {
        reason: "connection failed for postgres://admin:secret@db.example/pnpr?sslmode=require and libsql://edge.example/pnpr?authToken=token-value".to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("connection failed"));
    assert!(message.contains("db.example"));
    assert!(message.contains("edge.example"));
    assert!(message.contains("sslmode=require"));
    assert!(message.contains("postgres://redacted@db.example/pnpr?sslmode=require"));
    assert!(message.contains("authToken=redacted"));
    assert!(!message.contains("admin"));
    assert!(!message.contains("secret"));
    assert!(!message.contains("token-value"));
}

#[test]
fn log_message_redacts_ipv6_database_url_credentials() {
    let err = RegistryError::Internal {
        reason: "connection failed for postgres://admin:secret@[::1]/pnpr?sslmode=require"
            .to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("postgres://redacted@[::1]/pnpr?sslmode=require"));
    assert!(!message.contains("admin"));
    assert!(!message.contains("secret"));
}

#[test]
fn log_message_redacts_malformed_database_url_credentials() {
    let err = RegistryError::Internal {
        reason:
            "connection failed for postgres://admin:sec#ret@db.example/pnpr?password=query-secret"
                .to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("postgres://redacted@db.example/pnpr?password=redacted"));
    assert!(!message.contains("admin"));
    assert!(!message.contains("sec#ret"));
    assert!(!message.contains("query-secret"));
}

#[test]
fn log_message_redacts_malformed_database_url_credentials_with_slash() {
    let err = RegistryError::Internal {
        reason: "connection failed for postgres://admin:pa/ss@db.example/pnpr".to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("postgres://redacted@db.example/pnpr"));
    assert!(!message.contains("admin"));
    assert!(!message.contains("pa/ss"));
}

#[test]
fn log_message_redacts_database_url_fragment_secrets() {
    let err = RegistryError::Internal {
        reason: "connection failed for postgres://db.example/pnpr#password=fragment-secret"
            .to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("postgres://db.example/pnpr"));
    assert!(!message.contains("fragment-secret"));
    assert!(!message.contains("#password"));
}

#[test]
fn log_message_redacts_malformed_database_url_fragment_secrets() {
    let err = RegistryError::Internal {
        reason:
            "connection failed for postgres://admin:pa/ss@db.example/pnpr#password=fragment-secret"
                .to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("postgres://redacted@db.example/pnpr"));
    assert!(!message.contains("admin"));
    assert!(!message.contains("pa/ss"));
    assert!(!message.contains("fragment-secret"));
    assert!(!message.contains("#password"));
}
