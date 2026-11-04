use super::{PublishedReleaseError, Release, confirm_published};
use crate::{
    parse_supported_registry_url, provenance_gen::ProvenanceSubject,
    registry_config_keys::NormalizedRegistryUrl,
};
use base64::Engine as _;
use pnpm_network::ThrottledClient;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ANY: &str = "sha512-YW55";
const LINUX: &str = "sha512-bGludXg=";

fn release(primary: Option<&str>, subjects: Option<Vec<ProvenanceSubject>>) -> Release<'static> {
    Release {
        name: "@acme/cli",
        version: "1.0.0",
        primary: primary.map(str::to_owned),
        variants: BTreeMap::from([("linux-x64".to_owned(), LINUX.to_owned())]),
        subjects,
        tag: "latest",
    }
}

/// The record pnpr keeps of `@acme/cli@1.0.0` with a `linux-x64` variant of
/// `variant`, and an `any` archive of `primary` when it has one.
fn record(primary: Option<&str>, variant: &str) -> Value {
    let mut record = json!({
        "purl": "pkg:npm/%40acme/cli@1.0.0",
        "variants": [{ "tag": "linux-x64", "inputs": { "archive": { "integrity": variant } } }],
    });
    if let Some(primary) = primary {
        record["inputs"] = json!({ "archive": { "kind": "tgz", "integrity": primary } });
    }
    record
}

fn subjects() -> Vec<ProvenanceSubject> {
    vec![
        ProvenanceSubject { name: "pkg:npm/%40acme/cli@1.0.0".into(), sha512_hex: "aa".into() },
        ProvenanceSubject {
            name: "pkg:npm/%40acme/cli@1.0.0?variant=linux-x64".into(),
            sha512_hex: "bb".into(),
        },
    ]
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
) -> Result<(), PublishedReleaseError> {
    let (_server, url) = registry(record, attestations, dist_tags).await;
    confirm_published(&ThrottledClient::default(), &url, None, release).await
}

fn difference(result: Result<(), PublishedReleaseError>) -> String {
    match result {
        Err(PublishedReleaseError::Differs { difference, .. }) => difference,
        other => panic!("expected a difference, got {other:?}"),
    }
}

#[tokio::test]
async fn a_release_recorded_with_the_same_archives_statement_and_tag_is_published() {
    let subjects = subjects();
    confirm(
        &release(Some(ANY), Some(subjects.clone())),
        &record(Some(ANY), LINUX),
        Some(&attestations(&subjects)),
        &json!({ "latest": "1.0.0" }),
    )
    .await
    .unwrap();
}

/// The record of a version without an `any` archive names no primary input:
/// the archive npm's documents point to is generated from the variants.
#[tokio::test]
async fn a_version_without_an_any_archive_is_compared_by_its_variants() {
    let latest = json!({ "latest": "1.0.0" });
    confirm(&release(None, None), &record(None, LINUX), None, &latest).await.unwrap();
    let other = record(None, "sha512-b3RoZXI=");
    assert!(
        difference(confirm(&release(None, None), &other, None, &latest).await)
            .contains("the variant `linux-x64` is recorded as `sha512-b3RoZXI=`"),
    );
}

#[tokio::test]
async fn a_release_with_other_archives_names_the_archive_that_differs() {
    let latest = json!({ "latest": "1.0.0" });
    let other_any = record(Some("sha512-b3RoZXI="), LINUX);
    assert!(
        difference(confirm(&release(Some(ANY), None), &other_any, None, &latest).await)
            .starts_with("the `any` archive is recorded as `sha512-b3RoZXI=`"),
    );

    let mut other_tags = record(Some(ANY), LINUX);
    other_tags["variants"][0]["tag"] = json!("darwin-arm64");
    assert_eq!(
        difference(confirm(&release(Some(ANY), None), &other_tags, None, &latest).await),
        "the version is recorded with the variants [darwin-arm64], and this publish has [linux-x64]",
    );
}

#[tokio::test]
async fn a_run_that_signs_needs_a_statement_naming_every_archive() {
    let latest = json!({ "latest": "1.0.0" });
    let signing = release(Some(ANY), Some(subjects()));
    let recorded = record(Some(ANY), LINUX);
    assert!(
        difference(confirm(&signing, &recorded, None, &latest).await)
            .contains("no provenance statement"),
    );
    let only_any = attestations(&subjects()[..1]);
    assert!(
        difference(confirm(&signing, &recorded, Some(&only_any), &latest).await)
            .contains("no provenance statement"),
    );
}

#[tokio::test]
async fn the_dist_tag_must_point_at_the_version() {
    let recorded = record(Some(ANY), LINUX);
    let elsewhere = json!({ "latest": "0.9.0" });
    assert_eq!(
        difference(confirm(&release(Some(ANY), None), &recorded, None, &elsewhere).await),
        "the dist-tag `latest` points at 0.9.0",
    );
    assert_eq!(
        difference(confirm(&release(Some(ANY), None), &recorded, None, &json!({})).await),
        "the dist-tag `latest` is not set",
    );
}
