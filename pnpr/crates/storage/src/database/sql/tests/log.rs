//! The transparency log of a namespace's release records: entries sequenced
//! with their versions, integrated into tiles and checkpoints in the hosted
//! store.

use super::{Db, config, packument, storage};
use crate::{
    DocumentWrite, HostedDocumentVersion, Storage,
    database::{BatchDocument, PublishNotes, ReleaseRecords},
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
    let database = storage.sql_database();
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
    let database = storage.sql_database();
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
    let database = storage.sql_database();
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
            .sql_database()
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
        .sql_database()
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

async fn checkpoint_ahead_without_entry_bundles_fails_closed(db: Db) {
    let donor_tmp = TempDir::new().unwrap();
    let donor = Storage::from_config(&config(&donor_tmp)).unwrap();
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&donor, "acme", "core", &versions(0..3)).await;
    publish(&donor, "acme", "core", &versions(3..5)).await;
    donor.integrate_log(&log, &signer).await.unwrap();
    let donor_checkpoint = donor
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap();
    let donor_hash_tile = donor.root
        .read_record(LOG_NAMESPACE, "npm/~acme/tile/0/000.p/5")
        .await
        .unwrap()
        .unwrap();

    let storage = storage(&db);
    publish(&storage, "acme", "core", &versions(0..3)).await;
    storage.integrate_log(&log, &signer).await.unwrap();
    overwrite_log_record(&storage, "npm/~acme/tile/0/000.p/5", &donor_hash_tile).await;
    overwrite_log_record(&storage, "npm/~acme/checkpoint", &donor_checkpoint.note).await;

    let behind_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(behind_err.to_string().contains("more than its stored entry bundles"), "{behind_err}");

    publish(&storage, "acme", "forked", &versions(0..4)).await;
    assert_eq!(
        storage
            .sql_database()
            .log_size(&log)
            .await
            .unwrap(),
        7,
    );
    let grown_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(grown_err.to_string().contains("more than its stored entry bundles"), "{grown_err}");
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

async fn unauthenticated_bundles_and_missing_prior_bundles_are_refused(db: Db) {
    let donor_tmp = TempDir::new().unwrap();
    let donor = Storage::from_config(&config(&donor_tmp)).unwrap();
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    publish(&donor, "acme", "core", &versions(0..2)).await;
    publish(&donor, "acme", "core", &versions(2..4)).await;
    donor.integrate_log(&log, &signer).await.unwrap();
    let uncheckpointed_bundle = donor.root
        .read_record(LOG_NAMESPACE, "npm/~acme/tile/entries/000.p/4")
        .await
        .unwrap()
        .unwrap();

    let storage = storage(&db);
    publish(&storage, "acme", "core", &versions(0..2)).await;
    storage.integrate_log(&log, &signer).await.unwrap();

    let no_key_err = storage.reconcile_log(&log, None).await.unwrap_err();
    assert!(
        no_key_err.to_string().contains("without a configured log signing key"),
        "{no_key_err}",
    );

    overwrite_log_record(&storage, "npm/~acme/tile/entries/000.p/4", &uncheckpointed_bundle).await;
    let unauth_err = storage.integrate_log(&log, &signer).await.unwrap_err();
    assert!(unauth_err.to_string().contains("authenticated entries"), "{unauth_err}");
    assert_eq!(
        storage
            .sql_database()
            .log_size(&log)
            .await
            .unwrap(),
        2,
    );
    storage.root.remove_record(LOG_NAMESPACE, "npm/~acme/tile/entries/000.p/4").await.unwrap();

    // Once entries 2..4 are sequenced in SQL, an in-flight bundle `000.p/4`
    // written before its checkpoint is authenticated by SQL and reconciles.
    publish(&storage, "acme", "core", &versions(2..4)).await;
    let sequenced_entries = storage
        .sql_database()
        .log_entries(&log, 0..4)
        .await
        .unwrap();
    let in_flight_bundle = pnpm_transparency_log::encode_bundle(&sequenced_entries).unwrap();
    overwrite_log_record(&storage, "npm/~acme/tile/entries/000.p/4", &in_flight_bundle).await;
    let verified = storage
        .reconcile_log(&log, Some(&signer))
        .await
        .unwrap();
    assert_eq!(verified.unwrap().tree_head.size, 2);

    let concurrent = storage
        .reconcile_after_concurrent_advance(&log, &signer, async {
            publish(&storage, "acme", "core", &versions(4..600)).await;
            assert_eq!(storage.integrate_log(&log, &signer).await.unwrap().size, 600);
        })
        .await
        .unwrap();
    assert!(concurrent.restarted);
    assert_eq!(concurrent.checkpoint.unwrap().tree_head.size, 600);
    assert_eq!(storage.integrate_log(&log, &signer).await.unwrap().size, 600);
    storage.root.remove_record(LOG_NAMESPACE, "npm/~acme/tile/entries/000").await.unwrap();
    let missing_err = storage
        .reconcile_log(&log, Some(&signer))
        .await
        .unwrap_err();
    assert!(missing_err.to_string().contains("tile/entries/000"), "{missing_err}");
}

#[tokio::test]
async fn multi_batch_checkpoint_recovery_extends_across_batches() {
    let tmp = TempDir::new().unwrap();
    let db_backup = TempDir::new().unwrap();
    let backup_file = db_backup.path().join("backup.sqlite");
    let signer = NoteSigner::from_private_key(SIGNING_KEY).unwrap();
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };

    let initial = Storage::from_config(&config(&tmp)).unwrap();
    publish(&initial, "acme", "core", &versions(0..10)).await;
    initial.integrate_log(&log, &signer).await.unwrap();
    rusqlite::Connection::open(tmp.path().join("metadata.sqlite"))
        .unwrap()
        .execute("VACUUM INTO ?1", [backup_file.to_str().unwrap()])
        .unwrap();

    publish(&initial, "acme", "core", &versions(10..4100)).await;
    let head_before = initial.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(head_before.size, 4100);
    drop(initial);

    let mut restored_cfg = config(&tmp);
    restored_cfg.database = pnpr_config::DatabaseBackend::Sqlite(backup_file);
    let restored = Storage::from_config(&restored_cfg).unwrap();
    let recovered = restored.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(recovered, head_before);
    publish(&restored, "acme", "core", &versions(4100..4102)).await;
    let extended = restored.integrate_log(&log, &signer).await.unwrap();
    assert_eq!(extended.size, 4102);
}

conformance! {
    an_empty_log_signs_no_checkpoint,
    altered_tiles_are_never_signed_over,
    checkpoint_ahead_without_entry_bundles_fails_closed,
    forked_checkpoint_is_never_signed_over,
    older_database_reimports_bundled_entries_and_extends_cleanly,
    records_are_sequenced_with_their_versions_and_integrated_after,
    a_removed_version_keeps_its_log_entry,
    unauthenticated_bundles_and_missing_prior_bundles_are_refused,
}
