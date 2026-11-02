use super::{
    attachments::{AttachmentKind, PreparedAttachment, TarballStorage, write_attachment_slot},
    manifest::record_publisher,
};
use base64::Engine;
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use pnpr_storage::{Storage, publish::PendingAttachment};
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
    let storage =
        Storage::new(&HostedStoreConfig::Fs, temp.path().join("store"), temp.path().join("cache"))
            .unwrap();
    let name = CanonicalPackageName::parse("pkg", Ecosystem::Npm).unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"tar bytes").unwrap();
    let tgz = encoder.finish().unwrap();
    let prepared = PreparedAttachment {
        attachment: PendingAttachment {
            filename: "pkg-1.0.0.tgz".to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(&tgz),
            declared_length: Some(tgz.len() as u64),
        },
        canonical: "pkg-1.0.0.tgz".to_string(),
        version: "1.0.0".to_string(),
        dist: json!({ "integrity": sha512(&tgz).to_string() }),
        kind: AttachmentKind::Tarball,
    };
    let decodes = Arc::new(tokio::sync::Semaphore::new(0));
    let write = write_attachment_slot(&storage, &name, prepared, TarballStorage::Tgz, &decodes);
    tokio::pin!(write);

    let waited = tokio::time::timeout(Duration::from_millis(200), &mut write).await;
    assert!(waited.is_err(), "the write must wait for a decode permit");
    decodes.add_permits(1);
    let (_slot, tar) = write.await.unwrap();
    assert!(tar.is_some());
    let integrities = storage.read_tar_integrities(&name).await.unwrap();
    assert_eq!(
        integrities
            .get(&sha512(&tgz).to_string())
            .map(|derived| derived.tar_integrity.as_str()),
        Some(sha512(b"tar bytes").to_string().as_str()),
    );
    assert_eq!(decodes.available_permits(), 1);
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

#[tokio::test]
async fn archives_compare_a_bounded_chunk_at_a_time() {
    let tmp = tempfile::TempDir::new().unwrap();
    let staged = tmp.path().join("staged");
    // Longer than one comparison chunk, so the comparison spans several.
    let bytes: Vec<u8> = (0..200_000u32)
        .map(|index| (index % 251) as u8)
        .collect();
    std::fs::write(&staged, &bytes).unwrap();
    let mut changed = bytes.clone();
    changed[150_000] ^= 1;
    let cases = [
        (bytes.clone(), Some(bytes.len() as u64), true),
        (bytes.clone(), None, true),
        (changed, None, false),
        (bytes[..100].to_vec(), None, false),
        ([bytes.as_slice(), b"more"].concat(), None, false),
        (bytes.clone(), Some(1), false),
    ];
    for (index, (existing, length, same)) in cases.into_iter().enumerate() {
        let body = axum::body::Body::from(existing);
        let compared = super::atomic::same_bytes(body, length, &staged).await.unwrap();
        assert_eq!(compared, same, "case {index}");
    }
}
