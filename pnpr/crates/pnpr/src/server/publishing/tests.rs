use super::{
    attachments::{AttachmentKind, PreparedAttachment, write_attachment_slot},
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
    let write = write_attachment_slot(&storage, &name, prepared, &decodes);
    tokio::pin!(write);

    let waited = tokio::time::timeout(Duration::from_millis(200), &mut write).await;
    assert!(waited.is_err(), "the write must wait for a decode permit");
    decodes.add_permits(1);
    let (_slot, tar) = write.await.unwrap();
    assert_eq!(tar.map(|tar| tar.integrity), Some(sha512(b"tar bytes")));
    assert_eq!(decodes.available_permits(), 1);
}

fn sha512(bytes: &[u8]) -> ssri::Integrity {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result()
}
