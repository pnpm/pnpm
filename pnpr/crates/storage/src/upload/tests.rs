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

    let upload = storage.begin_blob_upload().await.unwrap();
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

    let upload = storage.begin_blob_upload().await.unwrap();
    let id = upload.id().to_string();
    assert!(storage.open_blob_upload(&id).await.unwrap().is_some());

    assert!(storage.abort_blob_upload(&id).await.unwrap());
    assert!(storage.open_blob_upload(&id).await.unwrap().is_none());
    assert!(!storage.abort_blob_upload(&id).await.unwrap());
}

#[tokio::test]
async fn a_finished_upload_becomes_a_hosted_blob() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    let upload = storage.begin_blob_upload().await.unwrap();
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
        assert!(storage.open_blob_upload(id).await.unwrap().is_none());
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
    storage.begin_blob_upload().await.unwrap();

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app"]);
}

#[tokio::test]
async fn a_package_nested_under_another_is_not_listed() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);

    // `acme/app` is both a package and the namespace of `acme/app/tool`. The
    // walk stops at the first document so that a listing stays proportional
    // to the number of packages rather than the number of blobs, which leaves
    // the nested one unlisted. Tracked in pnpm/pnpm#14630.
    for name in ["acme/app", "acme/app/tool"] {
        storage.write_hosted_document_if_current(&image(name), b"{}", None).await.unwrap();
    }

    assert_eq!(storage.hosted_package_names().await.unwrap(), ["acme/app"]);
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

    let fresh = storage.begin_blob_upload().await.unwrap();
    let stale = storage.begin_blob_upload().await.unwrap();

    // Nothing is old enough yet.
    assert_eq!(storage.sweep_blob_uploads(Duration::from_hours(1)).await.unwrap(), 0);
    assert!(storage.open_blob_upload(stale.id()).await.unwrap().is_some());

    // Every upload counts as idle once the age is zero.
    assert_eq!(storage.sweep_blob_uploads(Duration::ZERO).await.unwrap(), 2);
    assert!(storage.open_blob_upload(fresh.id()).await.unwrap().is_none());
    assert!(storage.open_blob_upload(stale.id()).await.unwrap().is_none());

    // A sweep with nothing to reclaim is not an error.
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
