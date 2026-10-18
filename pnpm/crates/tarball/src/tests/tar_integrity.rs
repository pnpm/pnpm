use super::{
    AuthHeaders, Integrity, SilentReporter, StoreDir, TarballError, ThrottledClient, assert_eq,
    fast_retry_opts, fetch_and_extract_with_retry, gzip_bytes,
    tar_resources::{large_tar, serve, sha512, small_tar},
    tempdir_with_leaked_path,
};
use crate::{ArchiveIntegrity, IntegrityKind, archive_integrity::ArchiveVerification};

async fn fetch(
    url: &str,
    integrity: Option<&Integrity>,
    kind: IntegrityKind,
    store_dir: &'static StoreDir,
) -> Result<ArchiveIntegrity, TarballError> {
    fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        url,
        &ArchiveVerification::new(integrity, kind),
        None,
        crate::TarLimits::default(),
        0,
        "tar-pkg@1.0.0",
        "",
        store_dir,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .map(|(integrity, _, _)| integrity)
}

async fn fetch_tgz(
    tgz: Vec<u8>,
    integrity: Option<&Integrity>,
    kind: IntegrityKind,
) -> Result<ArchiveIntegrity, TarballError> {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (_server, _mock, url) = serve(tgz, None).await;
    fetch(&url, integrity, kind, store_dir).await
}

#[tokio::test]
async fn a_tgz_reports_the_integrity_of_its_tar() {
    for tar in [small_tar(), large_tar()] {
        let tgz = gzip_bytes(&tar);
        let integrity = fetch_tgz(tgz.clone(), Some(&sha512(&tgz)), IntegrityKind::Resource)
            .await
            .expect("the tgz must extract");
        assert_eq!(integrity, ArchiveIntegrity { resource: sha512(&tgz), tar: Some(sha512(&tar)) });
    }
}

#[tokio::test]
async fn a_tgz_is_checked_against_a_tar_pin() {
    for tar in [small_tar(), large_tar()] {
        let tgz = gzip_bytes(&tar);
        let integrity = fetch_tgz(tgz.clone(), Some(&sha512(&tar)), IntegrityKind::Tar)
            .await
            .expect("the tar pin must verify");
        assert_eq!(integrity.tar, Some(sha512(&tar)));

        let error = fetch_tgz(tgz.clone(), Some(&sha512(&tgz)), IntegrityKind::Tar)
            .await
            .expect_err("a tgz hash is not the tar's");
        assert!(matches!(error, TarballError::Checksum(_)), "{error:?}");
    }
}

#[tokio::test]
async fn a_resource_or_tar_pin_accepts_either_hash_only() {
    let tar = small_tar();
    let tgz = gzip_bytes(&tar);
    for pin in [sha512(&tgz), sha512(&tar)] {
        let integrity = fetch_tgz(tgz.clone(), Some(&pin), IntegrityKind::ResourceOrTar)
            .await
            .expect("either hash must verify");
        assert_eq!(integrity.tar, Some(sha512(&tar)));
    }
    let error = fetch_tgz(tgz, Some(&sha512(b"other")), IntegrityKind::ResourceOrTar)
        .await
        .expect_err("another hash must not verify");
    assert!(matches!(error, TarballError::Checksum(_)), "{error:?}");
}

/// Data after the gzip stream never fails a `.tgz` checked as the resource,
/// but leaves it without a tar.
#[tokio::test]
async fn a_tgz_with_trailing_data_has_no_tar() {
    let tar = small_tar();
    let mut tgz = gzip_bytes(&tar);
    tgz.extend_from_slice(b"trailing garbage");

    let integrity = fetch_tgz(tgz.clone(), Some(&sha512(&tgz)), IntegrityKind::Resource)
        .await
        .expect("the resource pin must verify");
    assert_eq!(integrity.tar, None);

    let error = fetch_tgz(tgz, Some(&sha512(&tar)), IntegrityKind::Tar).await
        .expect_err("a tar pin needs the whole gzip stream");
    assert!(matches!(error, TarballError::IncompleteTar { .. }), "{error:?}");
}

#[tokio::test]
async fn a_tar_split_across_gzip_members_is_hashed_whole() {
    let tar = small_tar();
    let (head, tail) = tar.split_at(tar.len() / 2);
    let mut tgz = gzip_bytes(head);
    tgz.extend(gzip_bytes(tail));

    let integrity = fetch_tgz(tgz.clone(), Some(&sha512(&tar)), IntegrityKind::Tar)
        .await
        .expect("every member is part of the tar");
    assert_eq!(integrity, ArchiveIntegrity { resource: sha512(&tgz), tar: Some(sha512(&tar)) });
}

/// Checked as the resource, a `.tgz` installs from its first gzip member on
/// both the in-memory and the streaming path, and a later member leaves it
/// without a tar.
#[tokio::test]
async fn a_resource_pin_extracts_the_first_member_only() {
    for tar in [small_tar(), large_tar()] {
        let mut tgz = gzip_bytes(&tar);
        tgz.extend(gzip_bytes(b"later member"));
        let integrity = fetch_tgz(tgz.clone(), Some(&sha512(&tgz)), IntegrityKind::Resource)
            .await
            .expect("the first member is a complete tar");
        assert_eq!(integrity.tar, None);
    }
}

/// A `file:` tarball read as the resource keeps tolerating data after its
/// gzip stream; only its tar's hash needs the whole stream.
#[tokio::test]
async fn a_local_tarball_with_trailing_data_resolves_as_the_resource() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pkg.tgz");
    let mut tgz = gzip_bytes(&small_tar());
    tgz.extend_from_slice(b"trailing garbage");
    std::fs::write(&path, &tgz).unwrap();

    let metadata = crate::read_local_tarball_metadata(&path, IntegrityKind::Resource)
        .await
        .expect("the resource needs no tar");
    assert_eq!(metadata.integrity, sha512(&tgz));
    let error = crate::read_local_tarball_metadata(&path, IntegrityKind::Tar)
        .await
        .expect_err("a tar hash needs the whole gzip stream");
    assert!(matches!(error, TarballError::IncompleteTar { .. }), "{error:?}");
}
