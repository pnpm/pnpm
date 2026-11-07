use super::{
    attachments::{AttachmentKind, PreparedAttachment, TarballStorage, write_attachment_slot},
    manifest::record_publisher,
};
use base64::Engine;
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use pnpr_storage::{
    Storage,
    publish::{AttachmentPayload, PendingAttachment},
};
use serde_json::json;
use std::{io::Write, sync::Arc, time::Duration};

#[test]
fn publisher_attribution_comes_only_from_an_authenticated_identity() {
    let mut authenticated = json!({
        "versions": { "1.0.0": { "_npmUser": { "name": "mallory" } } },
    });
    record_publisher(&mut authenticated, &Identity::user("alice"));
    assert_eq!(authenticated["versions"]["1.0.0"]["_npmUser"]["name"], "alice");

    let mut anonymous = json!({
        "versions": { "1.0.0": { "_npmUser": { "name": "mallory" } } },
    });
    record_publisher(&mut anonymous, &Identity::Anonymous);
    assert!(anonymous["versions"]["1.0.0"].get("_npmUser").is_none());
}

#[tokio::test]
async fn a_published_tarball_hashes_its_tar_under_a_decode_permit() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(
        &HostedStoreConfig::Fs,
        temp.path().join("store"),
        temp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(temp.path().join("metadata.sqlite")),
    )
    .unwrap();
    let name = CanonicalPackageName::parse("pkg", Ecosystem::Npm).unwrap();
    let tar = package_tar(br#"{"name":"pkg","version":"1.0.0"}"#);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).unwrap();
    let tgz = encoder.finish().unwrap();
    let prepared = PreparedAttachment {
        attachment: PendingAttachment {
            filename: "pkg-1.0.0.tgz".to_string(),
            payload: AttachmentPayload::Inline(base64::engine::general_purpose::STANDARD.encode(
                &tgz,
            )),
            declared_length: Some(tgz.len() as u64),
            declared_integrity: None,
        },
        canonical: "pkg-1.0.0.tgz".to_string(),
        version: "1.0.0".to_string(),
        dist: json!({ "integrity": sha512(&tgz).to_string() }),
        kind: AttachmentKind::Tarball,
        variant_tag: None,
    };
    let decodes = Arc::new(tokio::sync::Semaphore::new(0));
    let write =
        write_attachment_slot((&storage, None), &name, prepared, TarballStorage::Tgz, &decodes);
    tokio::pin!(write);

    let waited = tokio::time::timeout(Duration::from_millis(200), &mut write).await;
    assert!(waited.is_err(), "the write must wait for a decode permit");
    decodes.add_permits(1);
    let (_slot, published) = write.await.unwrap();
    assert!(published.is_some());
    let integrities = storage.read_tar_integrities(&name).await.unwrap();
    assert_eq!(
        integrities
            .get(&sha512(&tgz).to_string())
            .map(|derived| derived.tar_integrity.as_str()),
        Some(sha512(&tar).to_string().as_str()),
    );
    assert_eq!(decodes.available_permits(), 1);
}

fn package_tar(manifest: &[u8]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    builder.append_data(&mut header, "package/package.json", manifest).unwrap();
    builder.into_inner().unwrap()
}

fn sha512(bytes: &[u8]) -> ssri::Integrity {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result()
}

#[test]
fn a_request_digest_hashes_the_canonical_json() {
    let body = json!({ "b": 1, "a": [{ "d": 2, "c": r#"x""# }], "e": null });
    let reordered = json!({ "e": null, "a": [{ "c": r#"x""#, "d": 2 }], "b": 1 });
    let digest = super::atomic::request_digest(&body, None);
    assert_eq!(digest, super::atomic::request_digest(&reordered, None));
    let canonical = br#"{"a":[{"c":"x\"","d":2}],"b":1,"e":null}"#;
    assert_eq!(digest, ssri::Integrity::from(canonical).to_string());
    // The same body sent to another package is another request.
    let targeted = super::atomic::request_digest(&body, Some("pkg"));
    assert_ne!(targeted, digest);
    assert_ne!(targeted, super::atomic::request_digest(&body, Some("other")));
    let canonical = br#"{"body":{"a":[{"c":"x\"","d":2}],"b":1,"e":null},"target":"pkg"}"#;
    assert_eq!(targeted, ssri::Integrity::from(canonical).to_string());
}

#[test]
fn a_python_version_note_is_keyed_by_the_normalized_version() {
    let mut entry = json!({ "name": "demo", "version": "01.0", "notes": "- first\n" });
    let note = super::notes::take_version_note(&mut entry, pnpr_registry::Ecosystem::Pypi)
        .unwrap()
        .unwrap();
    assert_eq!(note.version, "1.0");
    assert!(entry.get("notes").is_none());
}

#[test]
fn a_response_carries_a_bounded_size_of_notices() {
    let notices: Vec<String> = (0..20)
        .map(|index| format!("{index:03} {}", "x".repeat(196)))
        .collect();
    let mut response = axum::response::Response::new(axum::body::Body::empty());
    super::add_npm_notices(&mut response, &notices);
    let sent: Vec<&str> = response
        .headers()
        .get_all("npm-notice")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect();
    // Nine 200-byte notices fit beside the summary, which counts the rest.
    assert_eq!(sent.len(), 10);
    assert!(sent[8].starts_with("008 "));
    assert_eq!(sent[9], "11 more versions were accepted with a notice");
    let total: usize = sent
        .iter()
        .map(|value| value.len())
        .sum();
    assert!(total <= 2048, "{total}");
    let longest_summary = format!("{} more versions were accepted with a notice", usize::MAX);
    assert!(longest_summary.len() <= 64);
}

#[test]
fn a_tilde_in_the_package_name_does_not_make_a_variant_attachment() {
    let name = CanonicalPackageName::parse("a~b", Ecosystem::Npm).unwrap();
    assert!(super::variants::parse_variant_attachment(&name, "a~b-1.0.0.tgz").is_none());
    let (blob, version, label) =
        super::variants::parse_variant_attachment(&name, "a~b-1.0.0~musl.tgz")
            .expect("a variant attachment")
            .expect("a valid label");
    assert_eq!(
        (blob.as_str(), version.as_str(), label.as_str()),
        ("a~b-1.0.0~musl.tgz", "1.0.0", "musl"),
    );
}

#[test]
fn semver_validation_is_strict() {
    for valid in [
        "1.0.0",
        "1.0.0-alpha.1",
        "1.0.0+001",
        "1.0.0-alpha+001",
        "1.0.0+build.01",
        "1.0.0-18446744073709551616",
    ] {
        assert!(super::is_valid_semver(valid), "{valid:?} should be valid");
    }
    for invalid in [
        "01.0.0",
        "v1.0.0",
        "=1.0.0",
        "1.0.0-01",
        "1.0.0-0000000000000000000018446744073709551616",
        "1.0.0-\u{141}",
        " 1.0.0",
        "1.0",
        "1.0.0+",
        "1.0.0+a+b",
        "9007199254740992.0.0",
    ] {
        assert!(!super::is_valid_semver(invalid), "{invalid:?} should be invalid");
    }
}

mod fence;

mod names;
