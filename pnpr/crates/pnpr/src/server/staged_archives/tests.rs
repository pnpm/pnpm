use super::{PrincipalUploads, StagedReferences, archive_digest, names_owner, query_integrity};
use axum::http::Uri;
use pnpr_config::HostedStoreConfig;
use pnpr_storage::{StagedArchiveKey, Storage};
use std::time::Duration;

#[test]
fn only_an_owner_parameter_routes_a_blob_to_shared_artifacts() {
    let uri = |query: &str| format!("/-/pnpr/v0/artifacts/blob?{query}").parse::<Uri>().unwrap();
    assert!(names_owner(&uri("organization=acme&integrity=sha512-AAAA")));
    assert!(names_owner(&uri("publisher=pkg&integrity=sha512-AAAA")));
    assert!(!names_owner(&uri("integrity=sha512-AAAA")));
}

#[test]
fn a_staged_archive_names_exactly_one_integrity() {
    let uri = |query: &str| format!("/-/pnpr/v0/artifacts/blob?{query}").parse::<Uri>().unwrap();
    assert_eq!(query_integrity(&uri("integrity=sha512-AAAA")).unwrap(), "sha512-AAAA");
    assert!(query_integrity(&uri("")).is_err());
    assert!(query_integrity(&uri("integrity=sha512-AAAA&integrity=sha512-BBBB")).is_err());
}

#[test]
fn only_a_sha512_integrity_addresses_a_staged_archive() {
    assert!(archive_digest("sha1-AAAA").is_err());
    let digest = archive_digest(&format!("sha512-{}==", "A".repeat(86))).unwrap();
    assert_eq!(digest.len(), 128);
}

#[tokio::test]
async fn one_principal_holds_at_most_two_upload_permits() {
    let uploads = PrincipalUploads::default();
    let first = uploads.acquire("alice").await;
    let _second = uploads.acquire("alice").await;
    let _other = uploads.acquire("bob").await;

    let third = tokio::time::timeout(Duration::from_millis(50), uploads.acquire("alice")).await;
    assert!(third.is_err(), "a third upload of one principal waits");
    drop(first);
    let third = tokio::time::timeout(Duration::from_secs(5), uploads.acquire("alice")).await;
    assert!(third.is_ok(), "it proceeds once a permit is free");
}

#[tokio::test]
async fn a_principal_without_uploads_leaves_no_entry() {
    let uploads = PrincipalUploads::default();
    drop(uploads.acquire("alice").await);
    assert!(uploads.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn every_reference_to_a_staged_archive_counts_against_one_request() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = Storage::new(
        &HostedStoreConfig::Fs,
        tmp.path().join("storage"),
        tmp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    )
    .unwrap();
    let integrity = format!("sha512-{}==", "A".repeat(86));
    let digest = archive_digest(&integrity).unwrap();
    let key = StagedArchiveKey { principal: "alice", digest: &digest };
    storage
        .stage_archive(&key, vec![0; 10])
        .await
        .unwrap();

    let twice = vec![integrity.as_str(); 2];
    assert!(
        StagedReferences::check_within(&storage, "alice", twice.clone(), 20)
            .await
            .is_ok(),
    );
    let thrice = vec![integrity.as_str(); 3];
    let too_many = vec![integrity.as_str(); 257];
    let refused = StagedReferences::check_within(&storage, "alice", too_many, u64::MAX).await;
    assert!(refused.is_err_and(|err| err.to_string().contains("257 staged archives")));
    let refused = StagedReferences::check_within(&storage, "alice", thrice, 20).await;
    assert!(refused.is_err_and(|err| err.to_string().contains("30 bytes")));

    // The commit gives what it claimed back.
    let claimed = StagedReferences::check_within(&storage, "alice", twice, 20)
        .await
        .unwrap();
    claimed.release(&storage).await;
    assert_eq!(storage.claimable_staged_archive_size(&key).await.unwrap(), None);
    assert_eq!(storage.staged_archive_usage("alice").await.unwrap().bytes, 0);
}
