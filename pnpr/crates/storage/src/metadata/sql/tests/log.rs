//! The transparency log of a catalog's release records: entries sequenced
//! with their versions, integrated into tiles and checkpoints in the hosted
//! store.

use super::{Db, packument, storage};
use crate::{
    HostedDocumentVersion,
    metadata::{BatchDocument, PublishNotes, ReleaseRecords},
    transparency_log::LogId,
};
use pnpm_release_record::StoredRecord;
use pnpm_transparency_log::{
    CheckpointPolicy, KeySet, NoteSigner, TilePath, decode_bundle, verify_inclusion,
};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};

/// Go's `note` test key.
const SIGNING_KEY: &str =
    "PRIVATE+KEY+PeterNeumann+c74f20a3+AYEKFALVFGyNhPJEMzD1QIDr+Y7hfZx09iUvxdXHKDFz";

fn record_of(name: &str, version: &str) -> Vec<u8> {
    let record = json!({
        "schema": "pnpr-release-record/1",
        "purl": format!("pkg:npm/{name}@{version}"),
        "name": name,
        "version": version,
        "revision": 0,
        "interpretation": "npm-install-manifest/1",
        "manifest": {},
    });
    serde_json::to_vec(&record).unwrap()
}

/// Publish `versions` of `name` in `catalog`, each with its record, on top of
/// what the store holds of it.
async fn publish(storage: &crate::Storage, catalog: &str, name: &str, versions: &[String]) {
    let database = storage.sql_metadata();
    let org = storage.for_hosted(catalog, Ecosystem::Npm);
    let npm = CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap();
    let current = org.read_hosted_document_for_update(&npm).await.unwrap();
    let mut document: Value = match &current {
        Some(current) => serde_json::from_slice(&current.bytes).unwrap(),
        None => packument(name, &[]),
    };
    let mut records = ReleaseRecords::new();
    for version in versions {
        document["versions"][version] = json!({
            "name": name,
            "version": version,
            "dist": { "integrity": format!("sha512-{version}") },
            "_archiveManifest": { "profile": "npm-install-manifest/1", "packageJson": "sha512-x" },
        });
        records.insert(version.clone(), record_of(name, version));
    }
    let expected = current.map(|current| current.version);
    let document = BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        bytes: serde_json::to_vec(&document).unwrap(),
        expected: expected.or(None::<HostedDocumentVersion>),
        principal: None,
        records,
    };
    database
        .commit_publication(catalog, vec![document], Vec::new(), PublishNotes::default(), None)
        .await
        .unwrap();
}

fn versions(range: std::ops::Range<u32>) -> Vec<String> {
    range
        .map(|patch| format!("1.0.{patch}"))
        .collect()
}

async fn records_are_sequenced_with_their_versions_and_integrated_after(db: Db) {
    let storage = storage(&db);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { catalog: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&storage, "acme", "core", &versions(0..3)).await;
    publish(&storage, "other", "core", &versions(0..1)).await;
    let database = storage.sql_metadata();
    let core = CanonicalPackageName::parse("core", Ecosystem::Npm).unwrap();
    for (version, index) in [("1.0.0", 0), ("1.0.2", 2)] {
        let entry = database.release_record_entry(&core, "acme", version).await.unwrap();
        let digest = StoredRecord::parse(record_of("core", version))
            .unwrap()
            .digest()
            .to_string();
        assert_eq!(entry, Some((index, digest)), "{version}");
    }
    // Each catalog has a log of its own.
    let other = database.release_record_entry(&core, "other", "1.0.0").await.unwrap();
    assert_eq!(other.map(|(index, _)| index), Some(0));
    assert!(
        storage
            .log_checkpoint(&log)
            .await
            .unwrap()
            .is_none(),
    );

    let tree_head = storage.integrate_log(&log, &signer).await.unwrap();
    assert_eq!((tree_head.origin.as_str(), tree_head.size), ("PeterNeumann/npm/~acme", 3));
    let checkpoint = storage
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();
    let log_key = log
        .signer(&signer)
        .unwrap()
        .verifier_key()
        .clone();
    let policy = CheckpointPolicy::new(
        "PeterNeumann/npm/~acme",
        KeySet::new(vec![log_key], 1).unwrap(),
        KeySet::new(Vec::new(), 0).unwrap(),
    )
    .unwrap();
    assert_eq!(policy.verify(&checkpoint.note).unwrap().tree_head, tree_head);
    let bundle = storage
        .log_tile(&log, TilePath::Entries { index: 0, width: 3 })
        .await
        .unwrap()
        .unwrap();
    let entries = database
        .log_entries(&log, 0..3)
        .await
        .unwrap();
    assert_eq!(
        decode_bundle(&bundle).unwrap(),
        entries
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
    );

    // A second integration with nothing new keeps the checkpoint.
    assert_eq!(storage.integrate_log(&log, &signer).await.unwrap(), tree_head);
    assert_eq!(
        storage
            .log_checkpoint(&log)
            .await
            .unwrap()
            .unwrap()
            .note,
        checkpoint.note,
    );

    // Past a full tile, every entry is proven against the newer checkpoint.
    publish(&storage, "acme", "core", &versions(3..300)).await;
    let tree_head = storage.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(tree_head.size, 300);
    let entries = database
        .log_entries(&log, 0..300)
        .await
        .unwrap();
    for index in [0, 2, 255, 256, 299] {
        let inclusion = storage
            .log_inclusion(&log, index)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(inclusion.checkpoint.tree_head, tree_head);
        let entry = &entries[usize::try_from(index).unwrap()];
        verify_inclusion(&inclusion.proof, &tree_head, index, entry).unwrap();
    }
    assert!(
        storage
            .log_inclusion(&log, 300)
            .await
            .unwrap()
            .is_none(),
    );
    let full = storage.log_tile(&log, TilePath::Hashes { level: 0, index: 0, width: 256 }).await;
    assert!(full.unwrap().is_some());
}

async fn a_removed_version_keeps_its_log_entry(db: Db) {
    let storage = storage(&db);
    publish(&storage, "acme", "core", &versions(0..2)).await;
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    super::write(&org, "core", &packument("core", &[])).await;
    let log = LogId { catalog: "acme".to_string(), ecosystem: Ecosystem::Npm };
    let database = storage.sql_metadata();
    assert_eq!(database.log_size(&log).await.unwrap(), 2);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    assert_eq!(storage.integrate_log(&log, &signer).await.unwrap().size, 2);
}

/// A hash tile altered in the hosted store is refused before integration
/// extends the log over it, and no checkpoint is signed over it.
async fn altered_tiles_are_never_signed_over(db: Db) {
    let storage = storage(&db);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { catalog: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&storage, "acme", "core", &versions(0..3)).await;
    storage.integrate_log(&log, &signer).await.unwrap();
    let key = "npm/~acme/tile/0/000.p/3";
    let namespace = crate::transparency_log::LOG_NAMESPACE;
    let tile = storage.root
        .read_record(namespace, key)
        .await
        .unwrap()
        .unwrap();
    let mut altered = tile.clone();
    altered[0] ^= 1;
    let replaced = storage.root.replace_record_if_current(namespace, key, &tile, &altered);
    assert_eq!(replaced.await.unwrap(), crate::DocumentWrite::Written);

    publish(&storage, "acme", "core", &versions(3..5)).await;
    let err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(err.to_string().contains("disagree"), "{err}");
    let checkpoint = storage
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.tree_head.size, 3);
}

/// A log with no sequenced entry has the empty tree and no checkpoint.
async fn an_empty_log_signs_no_checkpoint(db: Db) {
    let storage = storage(&db);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { catalog: "acme".to_string(), ecosystem: Ecosystem::Npm };
    let tree_head = storage.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(tree_head.size, 0);
    assert_eq!(tree_head.hash, pnpm_transparency_log::EMPTY_HASH);
    assert!(
        storage
            .log_checkpoint(&log)
            .await
            .unwrap()
            .is_none(),
    );
}

conformance! {
    an_empty_log_signs_no_checkpoint,
    altered_tiles_are_never_signed_over,
    records_are_sequenced_with_their_versions_and_integrated_after,
    a_removed_version_keeps_its_log_entry,
}
