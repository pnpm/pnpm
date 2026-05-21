use pacquet_network::{AuthHeaders, ThrottledClient};

use super::{FetchFullMetadataOptions, fetch_full_metadata};

/// Fetches against a real `mockito` server that asserts the request
/// arrives with the *full*-metadata `Accept` header
/// (`application/json`) and the registry-keyed `Authorization`
/// header. The 200 body carries `time`, `_npmUser`, and
/// `dist.attestations` so the test also confirms the deserialization
/// surfaces those fields end-to-end.
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
    };

    let pkg = fetch_full_metadata("acme", &opts).await.expect("server returns 200");
    assert_eq!(pkg.name, "acme");
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    let version = pkg.versions.get("1.0.0").expect("version present");
    assert!(version.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref()).is_some());
    assert!(version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).is_some());
    mock.assert_async().await;
}

/// A 5xx response propagates as a [`super::FetchMetadataError::Network`]
/// rather than panicking or silently returning a default-valued
/// `Package`. Mirrors upstream's `fetchFullMetadataCached`
/// fail-closed behavior — the verifier surfaces the underlying
/// message as the violation reason.
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

/// Scoped names percent-encode the `/` between the `@scope` prefix
/// and the bare name so the registry routes the request as a
/// single path segment — matches upstream's `toUri`. The mockito
/// `match` rule below uses the encoded path; if the encoding
/// regresses (raw slash), mockito returns 501 and the test fails.
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
    };

    let pkg =
        fetch_full_metadata("@scope/pkg", &opts).await.expect("encoded scoped name reaches mock");
    assert_eq!(pkg.name, "@scope/pkg");
    mock.assert_async().await;
}

/// A 200 response with a malformed body surfaces as
/// [`super::FetchMetadataError::Decode`] (not `Network`), so the
/// install-side diagnostic code routes to `decode_error` rather
/// than `network_error`. Mirrors upstream's split between transport
/// and decode failures.
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
    };

    let err = fetch_full_metadata("acme", &opts).await.expect_err("malformed JSON must surface");
    assert!(
        matches!(err, super::FetchMetadataError::Decode { .. }),
        "expected Decode variant, got: {err:?}",
    );
    mock.assert_async().await;
}
