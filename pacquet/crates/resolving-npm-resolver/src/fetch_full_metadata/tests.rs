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
        .match_header("accept", "application/json")
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
    };

    let pkg = fetch_full_metadata("acme", &opts).await.expect("server returns 200");
    assert_eq!(pkg.name, "acme");
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
    let version = pkg.versions.get("1.0.0").expect("version present");
    assert!(version.npm_user.as_ref().and_then(|u| u.trusted_publisher.as_ref()).is_some());
    assert!(version.dist.attestations.as_ref().and_then(|a| a.provenance.as_ref()).is_some());
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
    };

    let err = fetch_full_metadata("acme", &opts).await.expect_err("503 must surface");
    let text = format!("{err:?}");
    assert!(text.contains("acme"), "error mentions the failing URL: {text}");
    mock.assert_async().await;
}
