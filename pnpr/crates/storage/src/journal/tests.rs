use super::{
    ApplyProgress, DocumentMerge, HostedDocuments, JOURNAL_DIR, JournaledPublish,
    JournaledRevisionRef, MANIFEST_FILE, Manifest, SealedTxn, cleanup_lost_tmp_paths, sync_dir,
};
use crate::{HostedRevisionRefWrite, Storage, TarballFinalize, publish::merge_journaled_packument};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use object_store::{ObjectStore, memory::InMemory};
use pnpr_config::HostedStoreConfig;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::PackageName;
use pnpr_registry::Ecosystem;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tempfile::tempdir;
use tokio::fs;

/// The npm half of what the server passes in, which is all these tests
/// commit.
struct NpmDocuments;

impl HostedDocuments for NpmDocuments {
    fn merge(&self, merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>> {
        merge_journaled_packument(&merge)
    }
}

/// A merge that records nothing, the way an ecosystem document merge answers
/// when every entry the transaction journaled is already stored.
struct RecordsNothing;

impl HostedDocuments for RecordsNothing {
    fn merge(&self, _merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// A merge that fails the first time it is asked and records nothing after:
/// an apply that never got the document written, whose retry finds an entry
/// that must therefore be someone else's.
#[derive(Default)]
struct FailsThenRecordsNothing {
    merges: AtomicUsize,
}

impl HostedDocuments for FailsThenRecordsNothing {
    fn merge(&self, _merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>> {
        if self.merges.fetch_add(1, Ordering::Relaxed) == 0 {
            return Err(RegistryError::Internal { reason: "merge failed".to_string() });
        }
        Ok(None)
    }
}

/// A merge that writes the first package it is asked about and fails on
/// `fails`, then records nothing for either: an apply that got one document
/// written before it stopped, whose retry finds both entries in place.
struct WritesOneThenFails {
    fails: &'static str,
    written: AtomicUsize,
    failed: AtomicUsize,
}

impl HostedDocuments for WritesOneThenFails {
    fn merge(&self, merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>> {
        if merge.name.as_str() == self.fails {
            if self.failed.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(RegistryError::Internal { reason: "merge failed".to_string() });
            }
            return Ok(None);
        }
        if self.written.fetch_add(1, Ordering::Relaxed) == 0 {
            return Ok(Some(merge.journaled.to_vec()));
        }
        Ok(None)
    }
}

/// A merge that fails every time, naming the attempt it failed on.
#[derive(Default)]
struct AlwaysFails {
    merges: AtomicUsize,
}

impl HostedDocuments for AlwaysFails {
    fn merge(&self, _merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>> {
        let attempt = self.merges.fetch_add(1, Ordering::Relaxed);
        Err(RegistryError::Internal { reason: format!("merge failed on attempt {attempt}") })
    }
}

/// A journaled npm publish of `packument` for `name`, with no staged blobs
/// unless the test adds them.
fn npm_publish<'publish>(
    name: &'publish PackageName,
    packument: &'publish [u8],
    revision_refs: &'publish [JournaledRevisionRef],
) -> JournaledPublish<'publish> {
    JournaledPublish {
        name,
        org: None,
        ecosystem: Ecosystem::Npm,
        document: packument,
        base_version: None,
        slots: &[],
        revision_refs,
    }
}

#[tokio::test]
async fn cleanup_keeps_a_lost_tmp_blob_when_journal_removal_is_not_durable() {
    let tmp = tempdir().unwrap();
    let tmp_path = tmp.path().join("conflicted.tmp");
    fs::write(&tmp_path, b"loser").await.unwrap();

    cleanup_lost_tmp_paths(&[tmp_path.as_path()], false).await;

    assert!(fs::try_exists(tmp_path).await.unwrap());
}

#[tokio::test]
async fn cleanup_removes_a_lost_tmp_blob_when_journal_removal_is_durable() {
    let tmp = tempdir().unwrap();
    let tmp_path = tmp.path().join("conflicted.tmp");
    fs::write(&tmp_path, b"loser").await.unwrap();

    cleanup_lost_tmp_paths(&[tmp_path.as_path()], true).await;

    assert!(!fs::try_exists(tmp_path).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn sync_dir_reports_success_for_a_directory() {
    let tmp = tempdir().unwrap();

    sync_dir(tmp.path()).await.unwrap();
}

#[tokio::test]
async fn commit_persists_revision_references() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let name = PackageName::parse("pkg").unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": "pkg",
        "versions": {},
    }))
    .unwrap();
    let digest = URL_SAFE_NO_PAD.encode([7_u8; 64]);
    let record = br#"{"package":"pkg","version":"1.0.0"}"#.to_vec();
    let revision_refs = [JournaledRevisionRef {
        filename: "pkg-1.0.0.tgz".to_string(),
        digest: digest.clone(),
        ref_id: "a".repeat(64),
        bytes: record.clone(),
    }];
    let entries = [npm_publish(&name, &packument, &revision_refs)];

    storage.publish_journal().commit(&storage, &entries, &NpmDocuments).await.unwrap();

    assert_eq!(storage.read_hosted_revision_refs(&digest).await.unwrap(), vec![record.clone()]);
    assert_eq!(
        storage
            .write_hosted_revision_ref(&digest, &"a".repeat(64), "later-owner", &record)
            .await
            .unwrap(),
        HostedRevisionRefWrite::Committed,
    );
}

#[tokio::test]
async fn commit_drops_a_version_that_cannot_reserve_a_revision_reference() {
    let tmp = tempdir().unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), tmp.path().join("cache"))
            .unwrap();
    let digest = URL_SAFE_NO_PAD.encode([7_u8; 64]);
    for index in 0..crate::MAX_HOSTED_REVISION_REFS {
        storage
            .write_hosted_revision_ref(&digest, &format!("{index:064x}"), "existing-owner", b"{}")
            .await
            .unwrap();
    }
    let name = PackageName::parse("pkg").unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": "pkg",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": { "tarball": "http://host/pkg/-/publisher-chosen-name.tgz" },
            },
        },
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2026-07-01T00:00:00.000Z" },
    }))
    .unwrap();
    let revision_refs = [JournaledRevisionRef {
        filename: "pkg-1.0.0.tgz".to_string(),
        digest: digest.clone(),
        ref_id: "f".repeat(64),
        bytes: br#"{"package":"pkg","version":"1.0.0"}"#.to_vec(),
    }];
    let entries = [npm_publish(&name, &packument, &revision_refs)];

    let outcome =
        storage.publish_journal().commit(&storage, &entries, &NpmDocuments).await.unwrap();

    assert_eq!(outcome.reference_limit, Some(crate::MAX_HOSTED_REVISION_REFS));
    let hosted = storage.read_hosted_packument(&name).await.unwrap().unwrap();
    let hosted: serde_json::Value = serde_json::from_slice(&hosted).unwrap();
    assert_eq!(hosted["versions"], json!({}));
    assert_eq!(hosted["dist-tags"], json!({}));
    assert_eq!(hosted["time"].get("1.0.0"), None);
    assert_eq!(
        storage.read_hosted_revision_refs(&digest).await.unwrap().len(),
        crate::MAX_HOSTED_REVISION_REFS,
    );
}

#[tokio::test]
async fn commit_only_removes_transaction_owned_references_for_a_dropped_version() {
    let tmp = tempdir().unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, tmp.path().join("hosted"), tmp.path().join("cache"))
            .unwrap();
    let transaction_owned_digest = URL_SAFE_NO_PAD.encode([5_u8; 64]);
    let previously_owned_digest = URL_SAFE_NO_PAD.encode([6_u8; 64]);
    let full_digest = URL_SAFE_NO_PAD.encode([7_u8; 64]);
    for index in 0..crate::MAX_HOSTED_REVISION_REFS {
        storage
            .write_hosted_revision_ref(
                &full_digest,
                &format!("{index:064x}"),
                "existing-owner",
                b"{}",
            )
            .await
            .unwrap();
    }
    let name = PackageName::parse("pkg").unwrap();
    let packument = serde_json::to_vec(&json!({
        "name": "pkg",
        "versions": { "1.0.0": { "version": "1.0.0" } },
    }))
    .unwrap();
    let ref_id = "f".repeat(64);
    let record = br#"{"package":"pkg","version":"1.0.0"}"#.to_vec();
    let revision_refs = [
        JournaledRevisionRef {
            filename: "pkg-1.0.0.tgz".to_string(),
            digest: transaction_owned_digest.clone(),
            ref_id: ref_id.clone(),
            bytes: record.clone(),
        },
        JournaledRevisionRef {
            filename: "pkg-1.0.0.tgz".to_string(),
            digest: previously_owned_digest.clone(),
            ref_id: ref_id.clone(),
            bytes: record.clone(),
        },
        JournaledRevisionRef {
            filename: "pkg-1.0.0.tgz".to_string(),
            digest: full_digest,
            ref_id: ref_id.clone(),
            bytes: record.clone(),
        },
    ];
    let entries = [npm_publish(&name, &packument, &revision_refs)];

    let txn = storage.publish_journal().seal(&entries).await.unwrap();
    let revision_ref_owner = txn.revision_ref_owner.clone();
    storage
        .write_hosted_revision_ref(&transaction_owned_digest, &ref_id, &revision_ref_owner, &record)
        .await
        .unwrap();
    storage
        .write_hosted_revision_ref(&previously_owned_digest, &ref_id, "previous-owner", &record)
        .await
        .unwrap();
    storage
        .commit_hosted_revision_ref(&previously_owned_digest, &ref_id, "previous-owner")
        .await
        .unwrap();
    txn.apply(&storage, &NpmDocuments, &mut ApplyProgress::default()).await.unwrap();

    assert_eq!(
        storage.read_hosted_revision_refs(&transaction_owned_digest).await.unwrap(),
        Vec::<Vec<u8>>::new(),
    );
    assert_eq!(
        storage.read_hosted_revision_refs(&previously_owned_digest).await.unwrap(),
        vec![record],
    );
    let hosted = storage.read_hosted_packument(&name).await.unwrap().unwrap();
    let hosted: serde_json::Value = serde_json::from_slice(&hosted).unwrap();
    assert_eq!(hosted["versions"], json!({}));
}

#[tokio::test]
async fn applying_preserves_a_blob_conflict_across_a_later_package_failure() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let conflicted_name = PackageName::parse("conflicted-pkg").unwrap();
    let later_name = PackageName::parse("later-pkg").unwrap();
    let filename = "conflicted-pkg-1.0.0.tgz";

    let winner = storage.reserve_hosted_tarball(&conflicted_name, filename).await.unwrap();
    fs::write(&winner.tmp_path, b"winner").await.unwrap();
    assert_eq!(storage.finalize_tarball_slot(winner).await.unwrap(), TarballFinalize::Written);

    let loser = storage.reserve_hosted_tarball(&conflicted_name, filename).await.unwrap();
    fs::write(&loser.tmp_path, b"loser").await.unwrap();
    let loser_tmp_path = loser.tmp_path.clone();
    let conflicted_slots = [loser];
    let conflicted_packument = serde_json::to_vec(&json!({
        "name": "conflicted-pkg",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://host/conflicted-pkg/-/publisher-chosen-name.tgz",
                    "integrity": "loser",
                },
            },
        },
        "dist-tags": { "latest": "1.0.0" },
        "time": {
            "1.0.0": "2026-07-01T00:00:00.000Z",
            "modified": "2026-07-01T00:00:00.000Z",
        },
    }))
    .unwrap();
    let entries = [
        JournaledPublish {
            slots: &conflicted_slots,
            ..npm_publish(&conflicted_name, &conflicted_packument, &[])
        },
        npm_publish(&later_name, b"not-json", &[]),
    ];
    let txn_dir = storage.publish_journal().seal(&entries).await.unwrap().dir;

    // Reopened the way startup recovery does: every document is merged, so
    // the second package's unparsable one fails the apply partway.
    drop(
        SealedTxn::reopen(txn_dir.clone())
            .unwrap()
            .apply(&storage, &NpmDocuments, &mut ApplyProgress::default())
            .await
            .unwrap_err(),
    );
    assert!(
        fs::try_exists(&loser_tmp_path).await.unwrap(),
        "충돌한 임시 tarball은 트랜잭션 재시도를 위해 남아 있어야 합니다",
    );
    assert!(
        fs::try_exists(&txn_dir).await.unwrap(),
        "뒤 패키지가 실패하면 journal이 재시도를 위해 남아 있어야 합니다",
    );

    let manifest: Manifest =
        serde_json::from_slice(&fs::read(txn_dir.join(MANIFEST_FILE)).await.unwrap()).unwrap();
    let later_packument = json!({
        "name": "later-pkg",
        "versions": {
            "2.0.0": { "version": "2.0.0" },
        },
    });
    let later =
        manifest.packages.iter().find(|package| package.name == later_name.as_str()).unwrap();
    fs::write(txn_dir.join(&later.document_file), serde_json::to_vec(&later_packument).unwrap())
        .await
        .unwrap();

    SealedTxn::reopen(txn_dir.clone())
        .unwrap()
        .apply(&storage, &NpmDocuments, &mut ApplyProgress::default())
        .await
        .unwrap();

    let conflicted_hosted = storage.read_hosted_packument(&conflicted_name).await.unwrap().unwrap();
    let conflicted_hosted: serde_json::Value = serde_json::from_slice(&conflicted_hosted).unwrap();
    assert_eq!(conflicted_hosted["versions"], json!({}));
    assert_eq!(conflicted_hosted["dist-tags"], json!({}));
    assert_eq!(conflicted_hosted["time"].get("1.0.0"), None);
    let later_hosted = storage.read_hosted_packument(&later_name).await.unwrap().unwrap();
    let later_hosted: serde_json::Value = serde_json::from_slice(&later_hosted).unwrap();
    assert_eq!(later_hosted["versions"]["2.0.0"]["version"], "2.0.0");
    #[cfg(unix)]
    assert!(
        !fs::try_exists(&loser_tmp_path).await.unwrap(),
        "내구성 있는 journal 삭제 뒤에는 충돌한 임시 tarball을 정리해야 합니다",
    );
    assert!(
        !fs::try_exists(&txn_dir).await.unwrap(),
        "완료된 트랜잭션은 journal을 제거해야 합니다",
    );
}

/// A commit whose merge records nothing reports the package, so the surface
/// can tell its publisher that what the store serves under that name is not
/// what they uploaded.
#[tokio::test]
async fn commit_reports_a_package_whose_merge_recorded_nothing() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let name = PackageName::parse("pkg").unwrap();
    let document = serde_json::to_vec(&json!({ "name": "pkg", "versions": {} })).unwrap();
    let entries = [npm_publish(&name, &document, &[])];
    storage.publish_journal().commit(&storage, &entries, &NpmDocuments).await.unwrap();

    // The document is on disk now, so this commit's `base_version: None` is
    // stale and the merge decides what to write.
    let outcome =
        storage.publish_journal().commit(&storage, &entries, &RecordsNothing).await.unwrap();

    assert_eq!(outcome.unrecorded, vec!["pkg".to_string()]);
    assert!(outcome.lost_blobs.is_empty());
}

/// An apply that fails before writing a document is re-run, and an entry the
/// retry then finds in place was recorded by another writer: this transaction
/// never wrote one, so the publisher hears about it.
#[tokio::test]
async fn commit_reports_an_entry_the_retry_found_recorded_by_another_writer() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let name = PackageName::parse("pkg").unwrap();
    let document = serde_json::to_vec(&json!({ "name": "pkg", "versions": {} })).unwrap();
    let entries = [npm_publish(&name, &document, &[])];
    storage.publish_journal().commit(&storage, &entries, &NpmDocuments).await.unwrap();

    let documents = FailsThenRecordsNothing::default();
    let outcome = storage.publish_journal().commit(&storage, &entries, &documents).await.unwrap();

    assert_eq!(documents.merges.load(Ordering::Relaxed), 2, "the failed apply is re-run");
    assert_eq!(outcome.unrecorded, vec!["pkg".to_string()]);
}

/// The entries the first attempt wrote itself are not reported: the retry
/// finds them in place because this transaction put them there, and calling
/// that a duplicate would tell a publisher their publish duplicated itself.
#[tokio::test]
async fn commit_does_not_report_what_its_own_first_attempt_wrote() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let written_name = PackageName::parse("written-pkg").unwrap();
    let failed_name = PackageName::parse("failed-pkg").unwrap();
    let written_document =
        serde_json::to_vec(&json!({ "name": "written-pkg", "versions": {} })).unwrap();
    let failed_document =
        serde_json::to_vec(&json!({ "name": "failed-pkg", "versions": {} })).unwrap();
    let entries = [
        npm_publish(&written_name, &written_document, &[]),
        npm_publish(&failed_name, &failed_document, &[]),
    ];
    // Both documents exist, so neither commit below can take the write-as-is
    // path and every package goes through the merge.
    storage.publish_journal().commit(&storage, &entries, &NpmDocuments).await.unwrap();

    let documents = WritesOneThenFails {
        fails: "failed-pkg",
        written: AtomicUsize::new(0),
        failed: AtomicUsize::new(0),
    };
    let outcome = storage.publish_journal().commit(&storage, &entries, &documents).await.unwrap();

    assert_eq!(outcome.unrecorded, vec!["failed-pkg".to_string()]);
}

/// When the retry fails too, the commit reports the failure that started it
/// and leaves the sealed entry behind, which is what startup recovery picks up.
#[tokio::test]
async fn commit_keeps_the_journal_entry_when_the_retry_fails_too() {
    let tmp = tempdir().unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let storage = Storage::new(
        &HostedStoreConfig::ObjectStore { store: object_store, prefix: String::new() },
        tmp.path().join("hosted"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let name = PackageName::parse("pkg").unwrap();
    let document = serde_json::to_vec(&json!({ "name": "pkg", "versions": {} })).unwrap();
    let entries = [npm_publish(&name, &document, &[])];
    storage.publish_journal().commit(&storage, &entries, &NpmDocuments).await.unwrap();

    let documents = AlwaysFails::default();
    let err = storage.publish_journal().commit(&storage, &entries, &documents).await.unwrap_err();

    assert_eq!(documents.merges.load(Ordering::Relaxed), 2);
    assert!(err.to_string().contains("attempt 0"), "{err}");
    let journal_entries: Vec<_> = std::fs::read_dir(tmp.path().join("cache").join(JOURNAL_DIR))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(journal_entries.len(), 1, "{journal_entries:?}");
}
