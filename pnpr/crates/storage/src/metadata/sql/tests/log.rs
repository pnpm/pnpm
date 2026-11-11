//! The transparency log of a namespace's release records: entries sequenced
//! with their versions, integrated into tiles and checkpoints in the hosted
//! store.

use super::{Db, config, packument, storage};
use crate::{
    DocumentWrite, HostedDocumentVersion, Storage,
    metadata::{BatchDocument, PublishNotes, ReleaseRecords},
    transparency_log::{LOG_NAMESPACE, LogId},
};
use pnpm_release_record::StoredRecord;
use pnpm_transparency_log::{
    CheckpointPolicy, KeySet, NoteSigner, TilePath, decode_bundle, verify_inclusion,
};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};
use tempfile::TempDir;

/// Go's `note` test key.
const SIGNING_KEY: &str =
    "PRIVATE+KEY+PeterNeumann+c74f20a3+AYEKFALVFGyNhPJEMzD1QIDr+Y7hfZx09iUvxdXHKDFz";

pub(super) fn record_of(name: &str, version: &str) -> Vec<u8> {
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

/// Publish `versions` of `name` in `namespace`, each with its record, on top
/// of what the store holds of it.
async fn publish(storage: &Storage, namespace: &str, name: &str, versions: &[String]) {
    try_publish(storage, namespace, name, versions).await.unwrap();
}

async fn try_publish(
    storage: &Storage,
    namespace: &str,
    name: &str,
    versions: &[String],
) -> pnpr_error::Result<()> {
    let database = storage.sql_metadata();
    let slice = storage.for_hosted(namespace, Ecosystem::Npm);
    let npm = CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap();
    let current = slice.read_hosted_document_for_update(&npm).await.unwrap();
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
        .commit_publication(namespace, vec![document], Vec::new(), PublishNotes::default(), None)
        .await
        .map(|_| ())
}

fn versions(range: std::ops::Range<u32>) -> Vec<String> {
    range
        .map(|patch| format!("1.0.{patch}"))
        .collect()
}

async fn records_are_sequenced_with_their_versions_and_integrated_after(db: Db) {
    let storage = storage(&db);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
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
    // Each namespace has a log of its own.
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
    let slice = storage.for_hosted("acme", Ecosystem::Npm);
    super::write(&slice, "core", &packument("core", &[])).await;
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    let database = storage.sql_metadata();
    let core = CanonicalPackageName::parse("core", Ecosystem::Npm).unwrap();
    assert!(
        database
            .release_record_entry(&core, "acme", "1.0.0")
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(database.log_size(&log).await.unwrap(), 2);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    assert_eq!(storage.integrate_log(&log, &signer).await.unwrap().size, 2);
}

/// A hash tile altered in the hosted store is refused before integration
/// extends the log over it, and no checkpoint is signed over it.
async fn altered_tiles_are_never_signed_over(db: Db) {
    let storage = storage(&db);
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&storage, "acme", "core", &versions(0..3)).await;
    storage.integrate_log(&log, &signer).await.unwrap();
    let key = "npm/~acme/tile/0/000.p/3";
    let record_dir = crate::transparency_log::LOG_NAMESPACE;
    let tile = storage.root
        .read_record(record_dir, key)
        .await
        .unwrap()
        .unwrap();
    let mut altered = tile.clone();
    altered[0] ^= 1;
    let replaced = storage.root.replace_record_if_current(record_dir, key, &tile, &altered);
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
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
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

async fn overwrite_log_record(storage: &Storage, key: &str, bytes: &[u8]) {
    if storage.root.create_record(LOG_NAMESPACE, key, bytes).await.unwrap() {
        return;
    }
    let existing = storage.root
        .read_record(LOG_NAMESPACE, key)
        .await
        .unwrap()
        .unwrap();
    let written =
        storage.root.replace_record_if_current(LOG_NAMESPACE, key, &existing, bytes).await.unwrap();
    assert_eq!(written, DocumentWrite::Written);
}

async fn copy_log_namespace(donor: &Storage, target: &Storage) {
    let keys = donor.root.list_record_keys(LOG_NAMESPACE).await.unwrap();
    for key in keys {
        let bytes = donor.root
            .read_record(LOG_NAMESPACE, &key)
            .await
            .unwrap()
            .unwrap();
        overwrite_log_record(target, &key, &bytes).await;
    }
}

/// Restoring an older database snapshot behind `.transparency-log/` re-imports
/// the bundled entries across full and partial bundles, fences off their
/// rolled-back npm versions, and extends the existing tree without stalling or
/// forking.
async fn older_database_reimports_bundled_entries_and_extends_cleanly(db: Db) {
    let donor_tmp = TempDir::new().unwrap();
    let donor = Storage::from_config(&config(&donor_tmp)).unwrap();
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&donor, "acme", "core", &versions(0..3)).await;
    publish(&donor, "acme", "core", &versions(3..300)).await;
    let donor_head = donor.integrate_log(&log, &signer).await.unwrap();
    let donor_checkpoint = donor
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(donor_head.size, 300);

    let storage = storage(&db);
    publish(&storage, "acme", "core", &versions(0..3)).await;
    assert_eq!(storage.integrate_log(&log, &signer).await.unwrap().size, 3);
    copy_log_namespace(&donor, &storage).await;

    let recovered = storage.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(recovered, donor_head);
    assert_eq!(
        storage
            .sql_metadata()
            .log_size(&log)
            .await
            .unwrap(),
        300,
    );
    assert_eq!(
        storage
            .log_checkpoint(&log)
            .await
            .unwrap()
            .unwrap()
            .note,
        donor_checkpoint.note,
    );

    let republish = try_publish(&storage, "acme", "core", &["1.0.299".to_string()]).await;
    assert!(
        matches!(republish, Err(RegistryError::CannotRepublishUnpublishedVersion { .. })),
        "{republish:?}",
    );

    publish(&storage, "acme", "core", &versions(300..305)).await;
    let extended = storage.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(extended.size, 305);
    let entries = storage
        .sql_metadata()
        .log_entries(&log, 0..305)
        .await
        .unwrap();
    for index in [0_u64, 2, 255, 299, 304] {
        let inclusion = storage
            .log_inclusion(&log, index)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(inclusion.checkpoint.tree_head, extended);
        let entry = &entries[usize::try_from(index).unwrap()];
        verify_inclusion(&inclusion.proof, &extended, index, entry).unwrap();
    }
}

/// Integration fails closed when `.transparency-log/` holds a checkpoint
/// larger than the database's log and the missing entry bundles are absent,
/// and never overwrites that checkpoint even if new publishes grow the
/// database past it.
async fn checkpoint_ahead_without_entry_bundles_fails_closed(db: Db) {
    let donor_tmp = TempDir::new().unwrap();
    let donor = Storage::from_config(&config(&donor_tmp)).unwrap();
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&donor, "acme", "core", &versions(0..5)).await;
    donor.integrate_log(&log, &signer).await.unwrap();
    let donor_checkpoint = donor
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();

    let storage = storage(&db);
    publish(&storage, "acme", "core", &versions(0..3)).await;
    storage.integrate_log(&log, &signer).await.unwrap();
    overwrite_log_record(&storage, "npm/~acme/checkpoint", &donor_checkpoint.note).await;

    let behind_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(behind_err.to_string().contains("more than its stored entry bundles"));

    publish(&storage, "acme", "forked", &versions(0..4)).await;
    assert_eq!(
        storage
            .sql_metadata()
            .log_size(&log)
            .await
            .unwrap(),
        7,
    );
    let grown_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(grown_err.to_string().contains("more than its stored entry bundles"));
    assert_eq!(
        storage
            .log_checkpoint(&log)
            .await
            .unwrap()
            .unwrap()
            .note,
        donor_checkpoint.note,
    );
}

/// Integration fails closed when the stored checkpoint is not a prefix of the
/// database's log, even after the database grows larger than that checkpoint.
async fn forked_checkpoint_is_never_signed_over(db: Db) {
    let donor_tmp = TempDir::new().unwrap();
    let donor = Storage::from_config(&config(&donor_tmp)).unwrap();
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&donor, "acme", "forked", &versions(0..2)).await;
    donor.integrate_log(&log, &signer).await.unwrap();
    let forked_checkpoint = donor
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();

    let storage = storage(&db);
    publish(&storage, "acme", "core", &versions(0..2)).await;
    storage.integrate_log(&log, &signer).await.unwrap();
    let valid_checkpoint = storage
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();
    let mut bad_sig = valid_checkpoint.note.clone();
    let last = bad_sig.len() - 2;
    bad_sig[last] ^= 0x01;
    overwrite_log_record(&storage, "npm/~acme/checkpoint", &bad_sig).await;
    let sig_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(sig_err.to_string().contains("failed verification"), "{sig_err}");

    overwrite_log_record(&storage, "npm/~acme/checkpoint", &forked_checkpoint.note).await;

    let equal_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(equal_err.to_string().contains("disagrees"));

    publish(&storage, "acme", "core", &versions(2..5)).await;
    let larger_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(
        larger_err.to_string().contains("disagrees")
            || larger_err.to_string().contains("not a prefix"),
    );
    assert_eq!(
        storage
            .log_checkpoint(&log)
            .await
            .unwrap()
            .unwrap()
            .note,
        forked_checkpoint.note,
    );
}

conformance! {
    an_empty_log_signs_no_checkpoint,
    altered_tiles_are_never_signed_over,
    checkpoint_ahead_without_entry_bundles_fails_closed,
    forked_checkpoint_is_never_signed_over,
    older_database_reimports_bundled_entries_and_extends_cleanly,
    records_are_sequenced_with_their_versions_and_integrated_after,
    a_removed_version_keeps_its_log_entry,
}
