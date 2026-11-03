use super::{PrincipalUploads, archive_digest, names_owner, query_integrity};
use axum::http::Uri;
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
