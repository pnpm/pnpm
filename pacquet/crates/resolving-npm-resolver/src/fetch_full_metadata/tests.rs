use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use std::time::Duration;

use super::{FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata};

/// Unwrap a [`FetchFullMetadataOutcome::Modified`], panicking on
/// `NotModified`. Used by the success-path tests below where the
/// mock always responds 200.
fn expect_modified(outcome: FetchFullMetadataOutcome) -> pacquet_registry::Package {
    match outcome {
        FetchFullMetadataOutcome::Modified(pkg) => *pkg,
        FetchFullMetadataOutcome::NotModified => {
            panic!("expected Modified outcome, got NotModified")
        }
    }
}

fn no_retry_opts() -> RetryOpts {
    RetryOpts { retries: 0, ..Default::default() }
}

fn fast_retry_opts() -> RetryOpts {
    RetryOpts {
        retries: 1,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(1),
        ..Default::default()
    }
}

#[tokio::test]
async fn fetch_full_metadata_targets_full_endpoint_with_auth() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "_npmUser": {
                    "name": "alice",
                    "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
                },
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz",
                    "attestations": {
                        "provenance": { "predicateType": "https://slsa.dev/provenance/v1" }
                    }
                }
            }
        }
    }"#;
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("authorization", "Bearer top-secret")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::from_creds_map(
        [(pacquet_network::nerf_dart(&registry), "Bearer top-secret".to_owned())],
        None,
    );
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: true,
        etag: None,
        modified: None,
        retry_opts: no_retry_opts(),
    };

    let pkg =
        expect_modified(fetch_full_metadata("acme", &opts).await.expect("server returns 200"));
    assert_eq!(pkg.name, "acme");
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    let version = pkg.versions.get("1.0.0").expect("version present");
    assert!(version.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref()).is_some());
    assert!(version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).is_some());
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_full_metadata_uses_package_scope_auth() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "name": "@scope/pkg",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2025-01-15T12:00:00.000Z",
        "versions": {
            "1.0.0": {
                "name": "@scope/pkg",
                "version": "1.0.0",
                "dist": {
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/@scope/pkg-1.0.0.tgz"
                }
            }
        }
    }"#;
    let mock = server
        .mock("GET", "/@scope%2Fpkg")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::from_creds_map(
        [(
            format!("{}@scope", pacquet_network::nerf_dart(&registry)),
            "Bearer scoped-token".to_owned(),
        )],
        None,
    );
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: false,
        etag: None,
        modified: None,
        retry_opts: no_retry_opts(),
    };

    let pkg = expect_modified(
        fetch_full_metadata("@scope/pkg", &opts).await.expect("server returns 200"),
    );
    assert_eq!(pkg.name, "@scope/pkg");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_full_metadata_surfaces_5xx_as_network_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(503).expect(1).create_async().await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: true,
        etag: None,
        modified: None,
        retry_opts: no_retry_opts(),
    };

    let err = fetch_full_metadata("acme", &opts).await.expect_err("503 must surface");
    assert!(
        matches!(err, super::FetchMetadataError::Network { .. }),
        "expected Network variant, got: {err:?}",
    );
    let text = format!("{err:?}");
    assert!(text.contains("acme"), "error mentions the failing URL: {text}");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_full_metadata_retries_transient_status() {
    let mut server = mockito::Server::new_async().await;
    let first = server.mock("GET", "/acme").with_status(503).expect(1).create_async().await;
    let body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let second = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: true,
        etag: None,
        modified: None,
        retry_opts: fast_retry_opts(),
    };

    let pkg = expect_modified(fetch_full_metadata("acme", &opts).await.expect("503 retries"));
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn fetch_full_metadata_encodes_scoped_name() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "name": "@scope/pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "@scope/pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/scope-pkg-1.0.0.tgz"
                }
            }
        }
    }"#;
    let mock = server
        .mock("GET", "/@scope%2Fpkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: true,
        etag: None,
        modified: None,
        retry_opts: no_retry_opts(),
    };

    let pkg = expect_modified(
        fetch_full_metadata("@scope/pkg", &opts).await.expect("encoded scoped name reaches mock"),
    );
    assert_eq!(pkg.name, "@scope/pkg");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_full_metadata_surfaces_decode_failure_distinctly() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body("definitely not JSON")
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: true,
        etag: None,
        modified: None,
        retry_opts: no_retry_opts(),
    };

    let err = fetch_full_metadata("acme", &opts).await.expect_err("malformed JSON must surface");
    assert!(
        matches!(err, super::FetchMetadataError::Decode { .. }),
        "expected Decode variant, got: {err:?}",
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_full_metadata_returns_not_modified_on_304() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"fresh""#)
        .match_header("if-modified-since", "Wed, 15 Jan 2025 12:00:00 GMT")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        full_metadata: true,
        etag: Some(r#"W/"fresh""#),
        modified: Some("Wed, 15 Jan 2025 12:00:00 GMT"),
        retry_opts: no_retry_opts(),
    };

    let outcome = fetch_full_metadata("acme", &opts).await.expect("304 must succeed");
    assert!(
        matches!(outcome, FetchFullMetadataOutcome::NotModified),
        "expected NotModified, got: {outcome:?}",
    );
    mock.assert_async().await;
}
