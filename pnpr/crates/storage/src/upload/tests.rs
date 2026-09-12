use crate::{BlobFinalize, Storage, upload::is_upload_id};
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::time::Duration;
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, tmp.path().join("storage"), tmp.path().join("cache"))
        .unwrap()
}

fn image(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, Ecosystem::Oci).unwrap()
}

#[tokio::test]
async fn an_upload_accumulates_across_appends() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let upload = storage.begin_blob_upload(&image("acme/app")).await.unwrap();
    assert_eq!(upload.offset().await.unwrap(), 0);

    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"hello ").await.unwrap();
    assert_eq!(writer.finish().await.unwrap(), 6);

    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"world").await.unwrap();
    assert_eq!(writer.finish().await.unwrap(), 11);
    assert_eq!(upload.offset().await.unwrap(), 11);
}

#[tokio::test]
async fn an_upload_is_reopened_by_id_and_dropped_on_abort() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let upload = storage.begin_blob_upload(&image("acme/app")).await.unwrap();
    let id = upload.id().to_string();
    assert!(storage.open_blob_upload(&image("acme/app"), &id).await.unwrap().is_some());

    assert!(storage.abort_blob_upload(&id).await.unwrap());
    assert!(storage.open_blob_upload(&image("acme/app"), &id).await.unwrap().is_none());
    assert!(!storage.abort_blob_upload(&id).await.unwrap());
}

#[tokio::test]
async fn a_finished_upload_becomes_a_hosted_blob() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let upload = storage.begin_blob_upload(&image("acme/app")).await.unwrap();
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"layer bytes").await.unwrap();
    writer.finish().await.unwrap();

    let name = image("acme/app");
    let slot = storage.stage_uploaded_blob(upload, &name, "sha256-abc").await.unwrap();
    assert_eq!(storage.finalize_blob_slot(slot).await.unwrap(), BlobFinalize::Written);

    let (_, size) = storage.open_hosted_blob(&name, "sha256-abc").await.unwrap().unwrap();
    assert_eq!(size, Some(11));
}

#[tokio::test]
async fn an_id_that_is_not_32_hex_never_reaches_the_filesystem() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    for id in ["../escape", "", "not-hex", &"a".repeat(31), &"A".repeat(32)] {
        assert!(!is_upload_id(id), "{id:?} should not be an upload id");
        assert!(storage.open_blob_upload(&image("acme/app"), id).await.unwrap().is_none());
        assert!(!storage.abort_blob_upload(id).await.unwrap());
    }
}

#[tokio::test]
async fn a_multi_component_repository_name_is_listed() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    for name in ["acme/app", "acme/team/tool", "alpine"] {
        storage.write_hosted_document_if_current(&image(name), b"{}", None).await.unwrap();
    }

    let mut listed = storage.hosted_package_names().await.unwrap();
    listed.sort();
    assert_eq!(listed, ["acme/app", "acme/team/tool", "alpine"]);
}

#[tokio::test]
async fn an_in_progress_upload_is_not_a_repository() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    storage.write_hosted_document_if_current(&image("acme/app"), b"{}", None).await.unwrap();
    storage.begin_blob_upload(&image("acme/app")).await.unwrap();

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app"]);
}

#[tokio::test]
async fn a_package_nested_under_another_is_listed() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    for name in ["acme/app", "acme/app/tool"] {
        storage.write_hosted_document_if_current(&image(name), b"{}", None).await.unwrap();
    }

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app", "acme/app/tool"]);
}

#[tokio::test]
async fn files_inside_a_package_directory_are_not_mistaken_for_packages() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = image("alpine");
    storage.write_hosted_document_if_current(&name, b"{}", None).await.unwrap();
    let slot = storage.reserve_hosted_blob(&name, "sha256-abc").await.unwrap();
    tokio::fs::write(&slot.tmp_path, b"layer").await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["alpine"]);
}

#[tokio::test]
async fn the_sweep_reclaims_only_uploads_that_have_gone_quiet() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let fresh = storage.begin_blob_upload(&image("acme/app")).await.unwrap();
    let stale = storage.begin_blob_upload(&image("acme/app")).await.unwrap();

    assert_eq!(storage.sweep_blob_uploads(Duration::from_hours(1)).await.unwrap(), 0);
    assert!(storage.open_blob_upload(&image("acme/app"), stale.id()).await.unwrap().is_some());

    // Every upload counts as idle once the age is zero.
    assert_eq!(storage.sweep_blob_uploads(Duration::ZERO).await.unwrap(), 2);
    assert!(storage.open_blob_upload(&image("acme/app"), fresh.id()).await.unwrap().is_none());
    assert!(storage.open_blob_upload(&image("acme/app"), stale.id()).await.unwrap().is_none());

    assert_eq!(storage.sweep_blob_uploads(Duration::ZERO).await.unwrap(), 0);
}

#[tokio::test]
async fn a_blob_finalized_before_any_document_does_not_break_the_listing() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    // An image push finalizes blobs before the manifest that records them, so
    // a repository directory can hold files and no document. Walking into one
    // of those files would fail the whole listing rather than skip it.
    let name = image("acme/half-pushed");
    let slot = storage.reserve_hosted_blob(&name, "sha256-abc").await.unwrap();
    tokio::fs::write(&slot.tmp_path, b"layer").await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();

    storage.write_hosted_document_if_current(&image("acme/complete"), b"{}", None).await.unwrap();

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/complete"]);
}

#[tokio::test]
async fn an_unmanifested_package_does_not_have_its_blobs_walked() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    // An image push finalizes blobs before the manifest that records them,
    // so this is an ordinary state, not a crafted one. Walking into it would
    // enumerate every blob on a path an anonymous listing reaches.
    let half_pushed = image("acme/half-pushed");
    for blob in ["sha256-aa", "sha256-bb", "sha256-cc"] {
        let slot = storage.reserve_hosted_blob(&half_pushed, blob).await.unwrap();
        tokio::fs::write(&slot.tmp_path, b"layer").await.unwrap();
        storage.finalize_blob_slot(slot).await.unwrap();
    }
    storage.write_hosted_document_if_current(&image("acme/complete"), b"{}", None).await.unwrap();

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/complete"]);
}

#[tokio::test]
async fn an_upload_belongs_to_the_repository_that_started_it() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let upload = storage.begin_blob_upload(&image("acme/app")).await.unwrap();
    let id = upload.id().to_string();

    // An id is a capability over the bytes its own client sent. A publisher
    // for another repository of the same organization who learns one must not
    // be able to finish it there.
    assert!(storage.open_blob_upload(&image("acme/other"), &id).await.unwrap().is_none());
    assert!(storage.open_blob_upload(&image("acme/app"), &id).await.unwrap().is_some());
}

#[tokio::test]
async fn a_swept_upload_takes_its_repository_record_with_it() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    storage.begin_blob_upload(&image("acme/app")).await.unwrap();

    assert_eq!(storage.sweep_blob_uploads(Duration::ZERO).await.unwrap(), 1);
    assert!(uploads_left(&tmp).is_empty(), "the sweep left {:?}", uploads_left(&tmp));
}

#[tokio::test]
async fn staging_an_upload_leaves_nothing_behind_it() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = image("acme/app");

    let upload = storage.begin_blob_upload(&name).await.unwrap();
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"layer bytes").await.unwrap();
    writer.finish().await.unwrap();
    storage.stage_uploaded_blob(upload, &name, "sha256-abc").await.unwrap();

    // Every successful push would otherwise leave one small file here
    // forever, and the sweep has no age to judge a record by.
    assert!(uploads_left(&tmp).is_empty(), "staging left {:?}", uploads_left(&tmp));
}

#[tokio::test]
async fn a_record_left_without_its_upload_is_reclaimed() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let upload = storage.begin_blob_upload(&image("acme/app")).await.unwrap();
    // What a push interrupted between the two removals leaves behind.
    std::fs::remove_file(upload.path()).unwrap();

    assert_eq!(storage.sweep_blob_uploads(Duration::from_hours(1)).await.unwrap(), 0);
    assert!(uploads_left(&tmp).is_empty(), "the sweep left {:?}", uploads_left(&tmp));
}

/// Two organizations of one object-store backend, which share a scratch root
/// and so are the pair a repository name alone cannot tell apart.
fn two_orgs_of_one_bucket(tmp: &TempDir) -> (Storage, Storage) {
    let hosted = HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::new(object_store::memory::InMemory::new()),
        prefix: String::new(),
    };
    let root = Storage::new(&hosted, tmp.path().join("storage"), tmp.path().join("cache")).unwrap();
    (root.for_hosted("first"), root.for_hosted("second"))
}

#[tokio::test]
async fn an_upload_does_not_cross_between_organizations() {
    let tmp = TempDir::new().unwrap();
    let (first, second) = two_orgs_of_one_bucket(&tmp);

    let upload = first.begin_blob_upload(&image("acme/app")).await.unwrap();
    let id = upload.id().to_string();

    // Both host `acme/app`, and both stage through the same directory. The
    // second must still not reach the first's bytes.
    assert!(second.open_blob_upload(&image("acme/app"), &id).await.unwrap().is_none());
    assert!(first.open_blob_upload(&image("acme/app"), &id).await.unwrap().is_some());
}

fn uploads_left(tmp: &TempDir) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(tmp.path().join("storage").join(".pnpr-uploads"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect()
}

fn replicas() -> (Storage, Storage, TempDir, TempDir) {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let objects: std::sync::Arc<dyn object_store::ObjectStore> =
        std::sync::Arc::new(object_store::memory::InMemory::new());
    let storage = |temp: &TempDir| Storage {
        hosted: std::sync::Arc::new(crate::s3::S3Store::new(
            std::sync::Arc::clone(&objects),
            "images/".into(),
            temp.path().join("scratch"),
        )),
        cached: crate::Store::new(temp.path().join("cache")),
    };
    (storage(&first), storage(&second), first, second)
}

#[tokio::test]
async fn s3_upload_resumes_on_another_replica_and_survives_loss_of_scratch() {
    let (first, second, first_disk, _second_disk) = replicas();
    let repository = image("acme/app");
    let upload = first.begin_blob_upload(&repository).await.unwrap();
    let id = upload.id().to_string();
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"hello ").await.unwrap();
    writer.finish().await.unwrap();
    drop(upload);
    drop(first_disk);
    let resumed = second.open_blob_upload(&repository, &id).await.unwrap().unwrap();
    assert_eq!(resumed.offset().await.unwrap(), 6);
    let mut writer = resumed.append().await.unwrap();
    writer.write_all(b"world").await.unwrap();
    assert_eq!(writer.finish().await.unwrap(), 11);
    resumed.materialize().await.unwrap();
    assert_eq!(tokio::fs::read(resumed.path()).await.unwrap(), b"hello world");
    second.finalize_uploaded_blob(resumed, &repository, "sha256-test").await.unwrap();
    assert_eq!(
        second.open_hosted_blob(&repository, "sha256-test").await.unwrap().unwrap().1,
        Some(11),
    );
    assert!(second.open_blob_upload(&repository, &id).await.unwrap().is_none());
}

#[tokio::test]
async fn s3_concurrent_append_and_stale_completion_cannot_overwrite_accepted_bytes() {
    let (first, second, _first_disk, _second_disk) = replicas();
    let repository = image("app");
    let original = first.begin_blob_upload(&repository).await.unwrap();
    let stale = second.open_blob_upload(&repository, original.id()).await.unwrap().unwrap();
    let mut winner = original.append().await.unwrap();
    let mut loser = stale.append().await.unwrap();
    winner.write_all(b"winner").await.unwrap();
    loser.write_all(b"loser").await.unwrap();
    winner.finish().await.unwrap();
    assert!(loser.finish().await.is_err());
    assert!(second.finalize_uploaded_blob(stale, &repository, "sha256-stale").await.is_err());
    assert!(second.open_hosted_blob(&repository, "sha256-stale").await.unwrap().is_none());
    let resumed = second.open_blob_upload(&repository, original.id()).await.unwrap().unwrap();
    resumed.materialize().await.unwrap();
    assert_eq!(tokio::fs::read(resumed.path()).await.unwrap(), b"winner");
}

#[tokio::test]
async fn s3_sessions_are_repository_and_namespace_bound_and_expire() {
    let (first, second, _first_disk, _second_disk) = replicas();
    let repository = image("app");
    let upload = first.begin_blob_upload(&repository).await.unwrap();
    assert!(second.open_blob_upload(&image("other"), upload.id()).await.unwrap().is_none());
    assert!(
        second
            .for_hosted("other")
            .open_blob_upload(&repository, upload.id())
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(second.sweep_blob_uploads(Duration::from_hours(24)).await.unwrap(), 0);
    assert_eq!(second.sweep_blob_uploads(Duration::ZERO).await.unwrap(), 1);
    assert!(first.open_blob_upload(&repository, upload.id()).await.unwrap().is_none());
    let mut stale = upload.append().await.unwrap();
    stale.write_all(b"too late").await.unwrap();
    assert!(stale.finish().await.is_err());
    assert_eq!(second.sweep_blob_uploads(Duration::ZERO).await.unwrap(), 0);
}

#[tokio::test]
async fn failed_promotion_keeps_shared_chunks_available_for_retry() {
    let (first, second, _first_disk, _second_disk) = replicas();
    let repository = image("app");
    let upload = first.begin_blob_upload(&repository).await.unwrap();
    let id = upload.id().to_string();
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"accepted").await.unwrap();
    writer.finish().await.unwrap();
    upload.remote.as_ref().unwrap().prepare_completion("sha256-test").await.unwrap();
    let slot = first.stage_uploaded_blob(upload, &repository, "sha256-test").await.unwrap();
    tokio::fs::remove_file(&slot.tmp_path).await.unwrap();
    assert!(first.finalize_blob_slot(slot).await.is_err());
    let resumed = second.open_blob_upload(&repository, &id).await.unwrap().unwrap();
    assert_eq!(resumed.offset().await.unwrap(), 8);
    second.finalize_uploaded_blob(resumed, &repository, "sha256-test").await.unwrap();
    assert!(first.open_blob_upload(&repository, &id).await.unwrap().is_none());
    let (body, _) = first.open_hosted_blob(&repository, "sha256-test").await.unwrap().unwrap();
    assert_eq!(axum::body::to_bytes(body, 100).await.unwrap().as_ref(), b"accepted");
}

#[tokio::test]
async fn empty_shared_chunks_reject_a_concurrently_changed_session() {
    let (first, second, _first_disk, _second_disk) = replicas();
    let repository = image("app");
    let upload = first.begin_blob_upload(&repository).await.unwrap();
    let resumed = second.open_blob_upload(&repository, upload.id()).await.unwrap().unwrap();
    let empty = upload.append().await.unwrap();
    let mut writer = resumed.append().await.unwrap();
    writer.write_all(b"new bytes").await.unwrap();
    writer.finish().await.unwrap();
    assert!(matches!(
        empty.finish().await,
        Err(pnpr_error::RegistryError::BlobUploadConflict { .. })
    ));
}

#[tokio::test]
async fn conflicting_blob_promotion_does_not_consume_the_shared_upload() {
    let (first, second, _first_disk, _second_disk) = replicas();
    let repository = image("app");
    let slot = first.reserve_hosted_blob(&repository, "sha256-test").await.unwrap();
    tokio::fs::write(&slot.tmp_path, b"conflicting").await.unwrap();
    first.finalize_blob_slot(slot).await.unwrap();
    let upload = first.begin_blob_upload(&repository).await.unwrap();
    let id = upload.id().to_string();
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"accepted").await.unwrap();
    writer.finish().await.unwrap();
    assert_eq!(
        first.finalize_uploaded_blob(upload, &repository, "sha256-test").await.unwrap(),
        BlobFinalize::Conflict,
    );
    let resumed = second.open_blob_upload(&repository, &id).await.unwrap().unwrap();
    assert_eq!(resumed.offset().await.unwrap(), 8);
    first.remove_blob(&repository, "sha256-test").await.unwrap();
    second.finalize_uploaded_blob(resumed, &repository, "sha256-test").await.unwrap();
}

#[tokio::test]
async fn prepared_completion_freezes_chunks_and_survives_replica_loss() {
    let (first, second, first_disk, _second_disk) = replicas();
    let repository = image("app");
    let upload = first.begin_blob_upload(&repository).await.unwrap();
    let id = upload.id().to_string();
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"accepted").await.unwrap();
    writer.finish().await.unwrap();
    let stale = second.open_blob_upload(&repository, &id).await.unwrap().unwrap();
    let mut stale_writer = stale.append().await.unwrap();
    stale_writer.write_all(b"racing").await.unwrap();
    upload.remote.as_ref().unwrap().prepare_completion("sha256-test").await.unwrap();
    assert!(stale_writer.finish().await.is_err());
    assert!(second.abort_blob_upload(&id).await.is_err());
    drop(upload);
    drop(first_disk);
    let resumed = second.open_blob_upload(&repository, &id).await.unwrap().unwrap();
    let mut writer = resumed.append().await.unwrap();
    writer.write_all(b"extra").await.unwrap();
    assert!(writer.finish().await.is_err());
    assert_eq!(resumed.append().await.unwrap().finish().await.unwrap(), 8);
    assert!(second.finalize_uploaded_blob(resumed, &repository, "sha256-other").await.is_err());
    assert!(second.open_hosted_blob(&repository, "sha256-other").await.unwrap().is_none());
    let resumed = second.open_blob_upload(&repository, &id).await.unwrap().unwrap();
    second.finalize_uploaded_blob(resumed, &repository, "sha256-test").await.unwrap();
    let (body, _) = second.open_hosted_blob(&repository, "sha256-test").await.unwrap().unwrap();
    assert_eq!(axum::body::to_bytes(body, 100).await.unwrap().as_ref(), b"accepted");
}
