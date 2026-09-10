use super::{
    AsyncWriteExt, BlobWrite, CanonicalPackageName, ErrorKind, HostedRevisionRefWrite,
    HostedStoreConfig, MAX_HOSTED_REVISION_REFS, RegistryError, Storage,
    atomic_write::create_tmp_file_with, fs,
};
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, tmp.path().join("storage"), tmp.path().join("cache"))
        .unwrap()
}

fn pkg(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm).unwrap()
}

#[test]
fn document_write_conflict_delay_caps_growth() {
    assert_eq!(super::document_write_conflict_delay(0).as_millis(), 5);
    assert_eq!(super::document_write_conflict_delay(1).as_millis(), 10);
    assert_eq!(super::document_write_conflict_delay(6).as_millis(), 250);
    assert_eq!(super::document_write_conflict_delay(32).as_millis(), 250);
}

#[tokio::test]
async fn hosted_revision_refs_roundtrip_in_the_org_namespace() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp).for_hosted("acme");
    let digest = "A".repeat(86);
    let ref_id = "b".repeat(64);

    assert_eq!(storage.read_hosted_revision_refs(&digest).await.unwrap(), Vec::<Vec<u8>>::new());
    storage
        .write_hosted_revision_ref(
            &digest,
            &ref_id,
            "owner-a",
            br#"{"package":"foo","version":"1.0.0"}"#,
        )
        .await
        .unwrap();
    assert_eq!(
        storage.read_hosted_revision_refs(&digest).await.unwrap(),
        vec![br#"{"package":"foo","version":"1.0.0"}"#.to_vec()],
    );
    assert_eq!(
        storage_in(&tmp).read_hosted_revision_refs(&digest).await.unwrap(),
        Vec::<Vec<u8>>::new(),
    );
}

#[tokio::test]
async fn hosted_revision_ref_paths_reject_noncanonical_segments() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);

    let invalid_digest = storage.read_hosted_revision_refs("../escape").await;
    assert!(invalid_digest.is_err());
    let invalid_ref =
        storage.write_hosted_revision_ref(&digest, "../escape", "owner-a", b"{}").await;
    assert!(invalid_ref.is_err());
}

#[tokio::test]
async fn hosted_revision_ref_writes_enforce_the_read_bound() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);
    for index in 0..MAX_HOSTED_REVISION_REFS {
        storage
            .write_hosted_revision_ref(&digest, &format!("{index:064x}"), "owner-a", b"{}")
            .await
            .unwrap();
    }

    let overflow = MAX_HOSTED_REVISION_REFS;
    let err = storage
        .write_hosted_revision_ref(&digest, &format!("{overflow:064x}"), "owner-a", b"{}")
        .await
        .unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
    assert!(matches!(
        err,
        RegistryError::RevisionReferenceLimit { limit } if limit == MAX_HOSTED_REVISION_REFS
    ));

    assert_eq!(
        storage
            .write_hosted_revision_ref(&digest, &"0".repeat(64), "owner-a", b"{}")
            .await
            .unwrap(),
        HostedRevisionRefWrite::AlreadyClaimed,
    );
    let stray_dir = tmp.path().join("storage/.revisions/sha512").join(&digest);
    fs::write(stray_dir.join("not-a-reference.json"), b"stray").await.unwrap();
    fs::write(stray_dir.join("interrupted.tmp"), b"stray").await.unwrap();
    let refs = storage.read_hosted_revision_refs(&digest).await.unwrap();
    assert_eq!(refs.len(), MAX_HOSTED_REVISION_REFS);
    assert!(refs.iter().all(|bytes| bytes == b"{}"));
}

#[tokio::test]
async fn concurrent_hosted_revision_ref_writes_cannot_exceed_the_limit() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let digest = "A".repeat(86);
    let mut writes = Vec::new();
    for index in 0..MAX_HOSTED_REVISION_REFS * 2 {
        let storage = storage.clone();
        let digest = digest.clone();
        writes.push(tokio::spawn(async move {
            storage
                .write_hosted_revision_ref(&digest, &format!("{index:064x}"), "owner-a", b"{}")
                .await
        }));
    }

    let mut written = 0;
    let mut rejected = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(HostedRevisionRefWrite::Claimed) => written += 1,
            Ok(outcome) => panic!("unexpected revision-reference write outcome: {outcome:?}"),
            Err(RegistryError::RevisionReferenceLimit { .. }) => rejected += 1,
            Err(err) => panic!("unexpected revision-reference write error: {err}"),
        }
    }
    assert_eq!(written, MAX_HOSTED_REVISION_REFS);
    assert_eq!(rejected, MAX_HOSTED_REVISION_REFS);
    assert_eq!(
        storage.read_hosted_revision_refs(&digest).await.unwrap().len(),
        MAX_HOSTED_REVISION_REFS,
    );
}

#[tokio::test]
async fn hosted_blob_under_non_directory_package_path_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("foo");
    let storage_root = tmp.path().join("storage");
    fs::create_dir_all(&storage_root).await.unwrap();
    fs::write(storage_root.join("foo"), b"not a directory").await.unwrap();

    let Err(err) = storage.open_hosted_blob(&name, "foo-1.0.0.tgz").await else {
        panic!("expected hosted blob open to fail");
    };
    match err {
        RegistryError::Io(err) => assert_eq!(err.kind(), ErrorKind::NotADirectory),
        other => panic!("expected I/O error, got {other:?}"),
    }
}

#[tokio::test]
async fn temp_file_creation_retries_existing_candidate_without_overwriting() {
    let tmp = TempDir::new().unwrap();
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    let occupied = tmp.path().join("foo-1.0.0.tgz.tmp.occupied");
    let retry = tmp.path().join("foo-1.0.0.tgz.tmp.retry");
    fs::write(&occupied, b"occupied").await.unwrap();

    let mut first = true;
    let (mut file, path) = create_tmp_file_with(&final_path, |_| {
        if std::mem::replace(&mut first, false) { occupied.clone() } else { retry.clone() }
    })
    .await
    .unwrap();
    assert_eq!(path, retry);

    file.write_all(b"new").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    assert_eq!(fs::read(&occupied).await.unwrap(), b"occupied");
    assert_eq!(fs::read(&retry).await.unwrap(), b"new");
}

#[cfg(unix)]
#[tokio::test]
async fn temp_file_creation_does_not_follow_symlink_candidate() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    let victim = tmp.path().join("victim");
    let symlink_path = tmp.path().join("foo-1.0.0.tgz.tmp.symlink");
    let retry = tmp.path().join("foo-1.0.0.tgz.tmp.retry");
    fs::write(&victim, b"victim").await.unwrap();
    symlink(&victim, &symlink_path).unwrap();

    let mut first = true;
    let (mut file, path) = create_tmp_file_with(&final_path, |_| {
        if std::mem::replace(&mut first, false) { symlink_path.clone() } else { retry.clone() }
    })
    .await
    .unwrap();
    assert_eq!(path, retry);

    file.write_all(b"new").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    assert_eq!(fs::read(&victim).await.unwrap(), b"victim");
    assert_eq!(fs::read(&retry).await.unwrap(), b"new");
    assert!(std::fs::symlink_metadata(&symlink_path).unwrap().file_type().is_symlink());
}

#[tokio::test]
async fn failed_blob_finalize_removes_tmp_file() {
    let tmp = TempDir::new().unwrap();
    let tmp_path = tmp.path().join("foo-1.0.0.tgz.tmp.test");
    let final_path = tmp.path().join("foo-1.0.0.tgz");
    fs::create_dir(&final_path).await.unwrap();
    fs::write(final_path.join("block-rename"), b"occupied").await.unwrap();

    let file = fs::File::create(&tmp_path).await.unwrap();
    let mut write = BlobWrite { file: Some(file), tmp_path: Some(tmp_path.clone()), final_path };
    write.write_all(b"tarball").await.unwrap();

    assert!(write.finalize().await.is_err());
    assert!(!tmp_path.exists(), "failed finalization must remove its temporary file");
}

#[tokio::test]
async fn package_index_migrates_nested_legacy_documents_and_ignores_removed_ones() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let root = tmp.path().join("storage");
    fs::create_dir_all(root.join("acme/app/tool")).await.unwrap();
    fs::write(root.join("acme/app/package.json"), b"{}").await.unwrap();
    fs::write(root.join("acme/app/tool/package.json"), b"{}").await.unwrap();
    storage.rebuild_package_index().await.unwrap();
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app", "acme/app/tool"]);
    fs::remove_file(root.join("acme/app/package.json")).await.unwrap();
    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app/tool"]);
}

#[tokio::test]
async fn package_index_removes_deleted_and_failed_package_entries() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("@scope/app");
    storage
        .update_hosted_document_with_retry(&name, 1, |_| Ok(Some(b"{}".to_vec())))
        .await
        .unwrap();
    let index = tmp.path().join("storage/.package-index/@scope");
    assert!(index.exists());
    storage.remove_package(&name).await.unwrap();
    assert!(!index.exists());
    fs::create_dir_all(tmp.path().join("storage/@scope/app/package.json")).await.unwrap();
    assert!(storage.hosted.write_document_if_current(&name, b"{}", None).await.is_err());
    assert!(!index.exists());
}

/// The local backend has no `ETag`, so its claim on a staged record compares
/// the bytes: only the writer whose copy is what the store holds wins.
#[tokio::test]
async fn a_staged_record_is_replaced_only_while_it_is_unchanged() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let stage_id = "11111111-2222-3333-4444-555555555555";
    storage.create_staged_meta(stage_id, br#"{"id":"stage"}"#).await.unwrap();

    let first = storage
        .replace_staged_meta_if_current(stage_id, br#"{"id":"stage"}"#, br#"{"id":"stage","a":1}"#)
        .await
        .unwrap();
    assert_eq!(first, super::DocumentWrite::Written);

    let second = storage
        .replace_staged_meta_if_current(stage_id, br#"{"id":"stage"}"#, br#"{"id":"stage","b":2}"#)
        .await
        .unwrap();
    assert_eq!(second, super::DocumentWrite::Conflict);
    assert_eq!(
        storage.read_staged_meta(stage_id).await.unwrap().as_deref(),
        Some(&br#"{"id":"stage","a":1}"#[..]),
    );

    assert!(storage.remove_staged(stage_id).await.unwrap());
    let removed = storage
        .replace_staged_meta_if_current(
            stage_id,
            br#"{"id":"stage","a":1}"#,
            br#"{"id":"stage","c":3}"#,
        )
        .await
        .unwrap();
    assert_eq!(removed, super::DocumentWrite::Conflict);
    assert!(storage.read_staged_meta(stage_id).await.unwrap().is_none());
}
