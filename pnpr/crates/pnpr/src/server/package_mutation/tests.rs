use super::rollback_hosted_packument;
use pnpr_config::HostedStoreConfig;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::Storage;
use tempfile::TempDir;

fn test_storage(temp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, temp.path().join("store"), temp.path().join("cache"))
        .unwrap()
}

#[tokio::test]
async fn rollback_restores_original_when_current_matches_written() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let name = CanonicalPackageName::parse("pkg-a", Ecosystem::Npm).unwrap();
    let original = br#"{"name":"pkg-a","version":"1.0.0"}"#;
    let written = br#"{"name":"pkg-a","version":"2.0.0"}"#;

    storage.write_hosted_document_if_current(&name, original, None).await.unwrap();
    let doc = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    storage
        .write_hosted_document_if_current(&name, written, Some(&doc.version))
        .await
        .unwrap();

    rollback_hosted_packument(&storage, &name, written, original).await;

    let restored = storage
        .read_hosted_document(&name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored, original);
}

#[tokio::test]
async fn rollback_aborts_when_document_concurrently_modified() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let name = CanonicalPackageName::parse("pkg-b", Ecosystem::Npm).unwrap();
    let original = br#"{"name":"pkg-b","version":"1.0.0"}"#;
    let written = br#"{"name":"pkg-b","version":"2.0.0"}"#;
    let concurrent = br#"{"name":"pkg-b","version":"3.0.0"}"#;

    storage.write_hosted_document_if_current(&name, original, None).await.unwrap();
    let doc = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    storage
        .write_hosted_document_if_current(&name, concurrent, Some(&doc.version))
        .await
        .unwrap();

    rollback_hosted_packument(&storage, &name, written, original).await;

    let preserved = storage
        .read_hosted_document(&name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preserved, concurrent);
}

#[tokio::test]
async fn rollback_aborts_when_document_deleted() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let name = CanonicalPackageName::parse("pkg-c", Ecosystem::Npm).unwrap();
    let original = br#"{"name":"pkg-c","version":"1.0.0"}"#;
    let written = br#"{"name":"pkg-c","version":"2.0.0"}"#;

    rollback_hosted_packument(&storage, &name, written, original).await;

    let current = storage.read_hosted_document(&name).await.unwrap();
    assert!(current.is_none());
}
