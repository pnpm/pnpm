use std::time::Duration;

use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
use tokio::net::TcpListener;

use super::RegistryError;

#[test]
fn log_message_preserves_scoped_package_urls() {
    assert_eq!(
        super::redact_url_credentials("https://registry.npmjs.org/@babel/cli"),
        "https://registry.npmjs.org/@babel/cli",
    );
}

#[test]
fn log_message_redacts_credentials_in_scoped_package_urls() {
    assert_eq!(
        super::redact_url_credentials("https://user:secret@registry.npmjs.org/@babel/cli"),
        "https://redacted@registry.npmjs.org/@babel/cli",
    );
}

#[test]
fn log_message_redacts_malformed_credentials_that_parse_as_a_port() {
    assert_eq!(
        super::redact_url_credentials("postgres://admin:123/secret@db.example/pnpr"),
        "postgres://redacted@db.example/pnpr",
    );
}

#[test]
fn log_message_redacts_malformed_credentials_in_scoped_package_urls() {
    assert_eq!(
        super::redact_url_credentials("https://user:pa/ss@registry.npmjs.org/@babel/cli"),
        "https://redacted@registry.npmjs.org/@babel/cli",
    );
}

#[test]
fn log_message_preserves_at_signs_in_package_url_paths() {
    for url in [
        "https://registry.npmjs.org/@babel/cli@7.0.0",
        "https://registry.example/pkg/-/pkg@1.0.0.tgz",
        "http://localhost:4873/@babel/cli",
    ] {
        assert_eq!(super::redact_url_credentials(url), url);
    }
}

#[test]
fn log_message_redacts_malformed_password_ending_in_a_slash() {
    assert_eq!(
        super::redact_url_credentials("https://user:secret/@registry.example/package"),
        "https://redacted@registry.example/package",
    );
}

#[test]
fn log_message_redacts_port_parsed_passwords_containing_a_query_or_fragment_delimiter() {
    for (url, expected) in [
        ("postgres://admin:123/se#cret@db.example/pnpr", "postgres://redacted"),
        ("postgres://admin:123/se?cret@db.example/pnpr", "postgres://redacted"),
        ("http://localhost:4873/x#token=a@secret", "http://redacted"),
    ] {
        assert_eq!(super::redact_url_credentials(url), expected);
    }
}

#[test]
fn log_message_drops_fragment_containing_an_at_sign() {
    assert_eq!(
        super::redact_url_credentials("https://db.example/pnpr#password=alice@fragment-secret"),
        "https://db.example/pnpr",
    );
}

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

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    let url = format!("http://{addr}/");
    let err = client
        .get(&url)
        .send()
        .await
        .unwrap_err();
    assert!(err.is_timeout(), "expected timeout error, got {err:?}");

    let registry_err = RegistryError::Upstream { url, source: err };
    assert_eq!(registry_err.status_code(), StatusCode::GATEWAY_TIMEOUT);
    let RegistryError::Upstream { url, source } = registry_err else {
        panic!("expected upstream error")
    };
    let body_err = RegistryError::UpstreamBody { url, source: std::io::Error::other(source) };
    assert_eq!(body_err.status_code(), StatusCode::GATEWAY_TIMEOUT);
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

/// Redacting a URL's credentials leaves a query that carries no secret
/// exactly as it was written, rather than re-encoding it.
#[test]
fn log_message_keeps_a_non_sensitive_query_verbatim() {
    let err = RegistryError::Internal {
        reason: "connection failed for postgres://admin:secret@db.example/pnpr?options=a%20b"
            .to_string(),
    };

    let message = err.log_message();

    assert!(message.contains("postgres://redacted@db.example/pnpr?options=a%20b"), "{message}");
    assert!(!message.contains("secret"));
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

    assert_eq!(message, "Internal error: connection failed for postgres://redacted");
}

#[test]
fn log_message_redacts_unparsable_urls_with_query_or_fragment_before_at_sign() {
    for (url, expected) in [
        ("https://host:bad/x#token=a@secret", "https://redacted"),
        ("postgres://admin:pa?ss@db.example/pnpr", "postgres://redacted"),
        ("https://host:bad/@scope/pkg?token=a@secret", "https://redacted"),
    ] {
        assert_eq!(super::redact_url_credentials(url), expected);
    }
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

#[test]
fn is_transient_upstream_error_only_for_availability_failures() {
    // 5xx is a transient availability signal (a refetch may recover), so a
    // stale cache entry may answer in the meantime.
    for status in [500, 502, 503, 504] {
        let err = RegistryError::UpstreamStatus {
            url: "https://up.example/foo".to_string(),
            status,
            body: String::new(),
        };
        assert!(err.is_transient_upstream_error(), "status {status} is transient (5xx/transport)");
    }

    // An open circuit is an availability failure too.
    let circuit_open = RegistryError::UpstreamUnavailable {
        upstream: "npmjs".to_string(),
        retry_after: std::time::Duration::from_secs(30),
    };
    assert!(circuit_open.is_transient_upstream_error());

    // Every 4xx is an authoritative response about this request — including
    // 429 (throttle) and 408 (timeout) — and must NOT be masked (e.g. by
    // serving a stale cache entry).
    for status in [400, 401, 403, 408, 410, 429, 451] {
        let err = RegistryError::UpstreamStatus {
            url: "https://up.example/foo".to_string(),
            status,
            body: String::new(),
        };
        assert!(!err.is_transient_upstream_error(), "status {status} is not transient (4xx)");
    }

    // A non-upstream error is never a transient availability failure.
    let config_error = RegistryError::InvalidConfig { reason: "x".to_string() };
    assert!(!config_error.is_transient_upstream_error());
}

/// Masked-existence responses disclose no detail about the hidden resource.
#[tokio::test]
async fn not_found_renders_as_a_json_404() {
    let response = RegistryError::NotFound.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"error":"Not Found","reason":"Not Found","code":"ERR_PNPR_NOT_FOUND"}),
    );
}

#[tokio::test]
async fn configuration_failures_hide_operator_details_from_clients() {
    let error = RegistryError::InvalidConfig {
        reason: "database libsql://database.example.test has a newer schema. Upgrade pnpr".into(),
    };
    assert_eq!(error.log_kind(), "invalid_config");
    assert!(error.log_message().contains("Upgrade pnpr"));
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response
            .extensions()
            .get::<super::PublicErrorMessage>()
            .unwrap()
            .0,
        "Internal Server Error",
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"error":"Internal Server Error","reason":"Internal Server Error","code":"ERR_PNPR_INVALID_CONFIG"}),
    );
}

#[test]
fn upstream_body_errors_preserve_gateway_status_and_log_kind() {
    for (kind, status) in [
        (std::io::ErrorKind::TimedOut, StatusCode::GATEWAY_TIMEOUT),
        (std::io::ErrorKind::UnexpectedEof, StatusCode::BAD_GATEWAY),
    ] {
        let error = RegistryError::UpstreamBody {
            url: "https://registry.example/artifact".to_string(),
            source: std::io::Error::new(kind, "body failed"),
        };
        assert_eq!(error.status_code(), status);
        assert_eq!(error.log_kind(), "upstream");
        assert!(error.is_transient_upstream_error());
    }
}

#[test]
fn circuit_open_response_names_upstream_and_rounds_retry_after_up() {
    use axum::response::IntoResponse;
    let error = RegistryError::UpstreamUnavailable {
        upstream: "example-upstream".to_string(),
        retry_after: std::time::Duration::from_millis(1250),
    };
    assert!(error.public_message().contains("example-upstream"));
    let shared = RegistryError::Shared(std::sync::Arc::new(error));
    assert!(shared.public_message().contains("example-upstream"));
    let response = shared.into_response();
    assert_eq!(response.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "2");
}

#[test]
fn database_targets_redact_driver_credentials_in_query_parameters() {
    let target = crate::redact_connection_url(
        "postgres://database.example.test/registry?user=example-user&sslpassword=example-secret&token=example-token&password=punctuated)secret&other=value",
    );
    for secret in ["example-user", "example-secret", "example-token", "punctuated", "secret"] {
        assert!(!target.contains(secret), "{target}");
    }
    assert!(target.contains("database.example.test/registry"), "{target}");
    assert!(target.contains("other=value"), "{target}");
}

#[test]
fn database_targets_hide_passwords_parsed_as_paths() {
    for target in [
        "postgres://localhost:1/example-secret@database.example.test/registry",
        "postgres://example:12345/@database.example.test/registry",
        "postgres://example:12345678/@database.example.test/registry",
    ] {
        assert_eq!(crate::redact_connection_url(target), "[redacted]", "{target}");
    }
}

#[test]
fn database_cause_urls_redact_driver_query_credentials() {
    let error = RegistryError::StorageUnavailable {
        reason: "connection failed for postgres://database.example.test/registry?sslpassword=example-secret&user=example-user".into(),
    };
    let message = error.log_message();
    assert!(message.contains("database.example.test/registry"), "{message}");
    assert!(!message.contains("example-secret"), "{message}");
    assert!(!message.contains("example-user"), "{message}");
}

#[test]
fn committed_password_errors_preserve_the_cause_and_account_identity() {
    let original = RegistryError::UpstreamUnavailable {
        upstream: "example-registry".to_string(),
        retry_after: Duration::from_secs(5),
    };
    let expected_status = original.status_code();
    let expected_public = original.public_message();
    let expected_kind = original.log_kind();
    let committed = RegistryError::PasswordChangeCommitted {
        user_id: "example-account".to_string(),
        source: Box::new(original),
    };
    assert_eq!(committed.status_code(), expected_status);
    assert_eq!(committed.public_message(), expected_public);
    assert_eq!(committed.log_kind(), expected_kind);
    assert!(committed.is_transient_upstream_error());
    assert_eq!(committed.committed_password_user(), Some("example-account"));
    let shared = RegistryError::Shared(std::sync::Arc::new(committed));
    assert_eq!(shared.committed_password_user(), Some("example-account"));
}

#[test]
fn committed_password_errors_redact_the_original_server_failure() {
    let error = RegistryError::PasswordChangeCommitted {
        user_id: "example-account".to_string(),
        source: Box::new(RegistryError::Internal {
            reason: "failed at https://example-user:private-secret@example.test".to_string(),
        }),
    };
    assert_eq!(error.public_message(), "Internal Server Error");
    assert!(!error.log_message().contains("private-secret"));
    assert!(error.log_message().contains("Password change committed"));
}

#[tokio::test]
async fn unavailable_registries_answer_503_with_a_stable_code() {
    let response = RegistryError::RegistriesUnavailable.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "ERR_PNPR_REGISTRIES_UNAVAILABLE");
}
