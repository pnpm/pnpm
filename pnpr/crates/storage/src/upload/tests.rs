use crate::{BlobFinalize, Storage, upload::is_upload_id};
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
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
