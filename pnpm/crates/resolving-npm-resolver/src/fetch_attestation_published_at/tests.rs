use pacquet_network::{AuthHeaders, ThrottledClient};

use super::{FetchAttestationOptions, fetch_attestation_published_at};

fn opts<'a>(
    registry: &'a str,
    http_client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
) -> FetchAttestationOptions<'a> {
    FetchAttestationOptions { registry, http_client, auth_headers }
}

#[tokio::test]
async fn finds_publish_time_from_single_bundle() {
    let mut server = mockito::Server::new_async().await;
    // 2024-01-01T00:00:00Z = 1704067200
    let body = r#"{
        "attestations": [
            {
                "bundle": {
                    "verificationMaterial": {
                        "tlogEntries": [{ "integratedTime": "1704067200" }]
                    }
                }
            }
        ]
    }"#;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(result.as_deref(), Some("2024-01-01T00:00:00.000Z"));
    mock.assert_async().await;
}

#[tokio::test]
async fn earliest_wins_across_multiple_bundles() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "attestations": [
            { "bundle": { "verificationMaterial": { "tlogEntries": [{ "integratedTime": "1735689600" }] } } },
            { "bundle": { "verificationMaterial": { "tlogEntries": [{ "integratedTime": "1704067200" }, { "integratedTime": "1735689600" }] } } }
        ]
    }"#;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(
        result.as_deref(),
        Some("2024-01-01T00:00:00.000Z"),
        "earliest integratedTime across all bundles wins",
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn returns_none_on_404() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(result, None);
    mock.assert_async().await;
}

#[tokio::test]
async fn returns_none_on_5xx() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(result, None);
    mock.assert_async().await;
}

#[tokio::test]
async fn returns_none_on_malformed_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{ "unrelated": true }"#)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(result, None);
    mock.assert_async().await;
}

#[tokio::test]
async fn accepts_integrated_time_as_number() {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{
        "attestations": [
            { "bundle": { "verificationMaterial": { "tlogEntries": [{ "integratedTime": 1704067200 }] } } }
        ]
    }"#;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(result.as_deref(), Some("2024-01-01T00:00:00.000Z"));
    mock.assert_async().await;
}

#[tokio::test]
async fn trims_trailing_slash_from_registry_root() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/npm/v1/attestations/acme@1.0.0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{ "attestations": [{ "bundle": { "verificationMaterial": { "tlogEntries": [{ "integratedTime": "1704067200" }] } } }] }"#,
        )
        .expect(1)
        .create_async()
        .await;
    // Mockito's `server.url()` already lacks a trailing slash; force one.
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let result = fetch_attestation_published_at(
        "acme",
        "1.0.0",
        &opts(&registry, &http_client, &auth_headers),
    )
    .await
    .expect("network ok");
    assert_eq!(result.as_deref(), Some("2024-01-01T00:00:00.000Z"));
    mock.assert_async().await;
}
