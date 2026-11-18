use super::{PublishedReleaseError, Release, confirm_published};
use crate::{
    parse_supported_registry_url, provenance_gen::ProvenanceSubject,
    registry_config_keys::NormalizedRegistryUrl,
};
use base64::Engine as _;
use pnpm_network::ThrottledClient;
use serde_json::{Value, json};

const ARCHIVE: &str = "sha512-YW55";

fn release(subject: Option<ProvenanceSubject>) -> Release<'static> {
    Release {
        name: "@acme/cli",
        version: "1.0.0",
        integrity: ARCHIVE.to_owned(),
        subject,
        tag: "latest",
    }
}

/// The record pnpr keeps of `@acme/cli@1.0.0` with an archive of
/// `integrity`.
fn record(integrity: &str) -> Value {
    json!({
        "schema": pnpm_release_record::RECORD_SCHEMA,
        "purl": "pkg:npm/%40acme/cli@1.0.0",
        "inputs": { "archive": { "kind": "tgz", "integrity": integrity } },
    })
}

fn subject() -> ProvenanceSubject {
    ProvenanceSubject { name: "pkg:npm/%40acme/cli@1.0.0".into(), sha512_hex: "aa".into() }
}

/// The attestations response for a statement naming `subjects`.
fn attestations(subjects: &[ProvenanceSubject]) -> Value {
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": subjects
            .iter()
            .map(|subject| json!({ "name": subject.name, "digest": { "sha512": subject.sha512_hex } }))
            .collect::<Vec<_>>(),
    });
    let payload = base64::engine::general_purpose::STANDARD.encode(statement.to_string());
    json!({ "attestations": [{ "bundle": { "dsseEnvelope": { "payload": payload } } }] })
}

/// pnpr holding `record`, `attestations` when given, and `dist_tags` for
/// `@acme/cli`.
async fn registry(
    record: &Value,
    attestations: Option<&Value>,
    dist_tags: &Value,
) -> (mockito::ServerGuard, NormalizedRegistryUrl) {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/@acme%2fcli/1.0.0+r0")
        .with_body(record.to_string())
        .create_async()
        .await;
    let attestations_mock = server.mock("GET", "/-/npm/v1/attestations/@acme%2fcli@1.0.0");
    match attestations {
        Some(body) => attestations_mock.with_body(body.to_string()),
        None => attestations_mock.with_status(404),
    }
    .create_async()
    .await;
    server
        .mock("GET", "/-/package/@acme%2fcli/dist-tags")
        .with_body(dist_tags.to_string())
        .create_async()
        .await;
    let url = parse_supported_registry_url(&format!("{}/", server.url())).unwrap().normalized_url;
    (server, url)
}

async fn confirm(
    release: &Release<'_>,
    record: &Value,
    attestations: Option<&Value>,
    dist_tags: &Value,
) -> Result<bool, PublishedReleaseError> {
    let (_server, url) = registry(record, attestations, dist_tags).await;
    confirm_published(&ThrottledClient::default(), &url, None, release).await
}

fn difference(result: Result<bool, PublishedReleaseError>) -> String {
    match result {
        Err(PublishedReleaseError::Differs { difference, .. }) => difference,
        other => panic!("expected a difference, got {other:?}"),
    }
}

#[tokio::test]
async fn a_release_recorded_with_the_same_archive_statement_and_tag_is_published() {
    let confirmed = confirm(
        &release(Some(subject())),
        &record(ARCHIVE),
        Some(&attestations(&[subject()])),
        &json!({ "latest": "1.0.0" }),
    )
    .await;
    assert!(matches!(confirmed, Ok(true)), "{confirmed:?}");
}

#[tokio::test]
async fn a_release_with_another_archive_names_what_differs() {
    let latest = json!({ "latest": "1.0.0" });
    let other = record("sha512-b3RoZXI=");
    assert_eq!(
        difference(confirm(&release(None), &other, None, &latest).await),
        "the archive is recorded as `sha512-b3RoZXI=`, and this publish has `sha512-YW55`",
    );
}

#[tokio::test]
async fn a_run_that_signs_needs_a_statement_naming_the_archive() {
    let latest = json!({ "latest": "1.0.0" });
    let signing = release(Some(subject()));
    let recorded = record(ARCHIVE);
    let differs = difference(confirm(&signing, &recorded, None, &latest).await);
    assert!(differs.contains("no provenance statement"), "{differs}");
    let other = ProvenanceSubject { sha512_hex: "bb".into(), ..subject() };
    let other = attestations(&[other]);
    let differs = difference(confirm(&signing, &recorded, Some(&other), &latest).await);
    assert!(differs.contains("no provenance statement"), "{differs}");
}

#[tokio::test]
async fn the_dist_tag_must_point_at_the_version() {
    let recorded = record(ARCHIVE);
    let elsewhere = json!({ "latest": "0.9.0" });
    assert_eq!(
        difference(confirm(&release(None), &recorded, None, &elsewhere).await),
        "the dist-tag `latest` points at 0.9.0",
    );
    assert_eq!(
        difference(confirm(&release(None), &recorded, None, &json!({})).await),
        "the dist-tag `latest` is not set",
    );
}

/// A version pnpr has no record of is not confirmed, and is no difference
/// either: the caller decides what that means.
#[tokio::test]
async fn a_version_pnpr_has_no_record_of_is_not_confirmed() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/@acme%2fcli/1.0.0+r0")
        .with_status(404)
        .create_async()
        .await;
    let url = parse_supported_registry_url(&format!("{}/", server.url())).unwrap().normalized_url;
    let confirmed =
        confirm_published(&ThrottledClient::default(), &url, None, &release(None)).await;
    assert!(matches!(confirmed, Ok(false)), "{confirmed:?}");
}

/// The version's derived projection, which the record route may serve
/// without a stored record, is no record.
#[tokio::test]
async fn a_projection_without_the_record_schema_is_no_record() {
    let mut projection = record(ARCHIVE);
    projection
        .as_object_mut()
        .unwrap()
        .remove("schema");
    let confirmed = confirm(&release(None), &projection, None, &json!({ "latest": "1.0.0" })).await;
    assert!(matches!(confirmed, Ok(false)), "{confirmed:?}");
}
