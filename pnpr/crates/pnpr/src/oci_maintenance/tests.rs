use super::{collect, referenced_blobs};
use crate::RegistryError;
use pnpr_config::HostedStoreConfig;
use pnpr_oci::{Digest, ImageDocument, ManifestEntry, media_type};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::Storage;
use serde_json::json;
use std::{collections::HashSet, time::Duration};
use tempfile::TempDir;

const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;

fn setup() -> (TempDir, Storage) {
    let temp = TempDir::new().unwrap();
    let storage =
        Storage::new(&HostedStoreConfig::Fs, temp.path().join("store"), temp.path().join("cache"))
            .unwrap();
    (temp, storage)
}

fn name(repository: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(repository, Ecosystem::Oci).unwrap()
}

async fn blob(storage: &Storage, repository: &CanonicalPackageName, bytes: &[u8]) -> Digest {
    let digest = Digest::of(bytes);
    let slot = storage.reserve_hosted_blob(repository, &digest.blob_filename()).await.unwrap();
    tokio::fs::write(&slot.tmp_path, bytes).await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();
    digest
}

async fn retained_image(storage: &Storage, repository: &CanonicalPackageName) -> (Digest, Digest) {
    let layer = blob(storage, repository, b"layer").await;
    let bytes = serde_json::to_vec(&json!({
        "schemaVersion": 2, "mediaType": media_type::OCI_IMAGE_MANIFEST,
        "config": {"digest": layer, "size": 5}, "layers": [],
    }))
    .unwrap();
    let manifest = blob(storage, repository, &bytes).await;
    let mut document = ImageDocument::new(repository.as_str());
    document.insert_manifest(ManifestEntry {
        referrer: None,
        digest: manifest.clone(),
        size: bytes.len() as u64,
        media_type: media_type::OCI_IMAGE_MANIFEST.into(),
    });
    storage.write_hosted_document_if_current(repository, &document.to_bytes(), None).await.unwrap();
    (manifest, layer)
}

#[tokio::test]
async fn collection_keeps_untagged_manifests_and_layers_and_finds_nested_orphans() {
    let (_temp, storage) = setup();
    let repository = name("acme/app");
    let (manifest, layer) = retained_image(&storage, &repository).await;
    blob(&storage, &repository, b"orphan").await;
    let nested = name("acme/app/tool");
    let nested_orphan = blob(&storage, &nested, b"nested orphan").await;
    assert_eq!(
        collect(&storage, Duration::from_hours(24), false, &HashSet::new(), MANIFEST_LIMIT)
            .await
            .unwrap(),
        (0, 0),
    );
    assert_eq!(
        collect(&storage, Duration::ZERO, true, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (2, 19),
    );
    assert!(
        storage.open_hosted_blob(&nested, &nested_orphan.blob_filename()).await.unwrap().is_some(),
    );
    assert_eq!(
        collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (2, 19),
    );
    assert!(
        storage.open_hosted_blob(&repository, &manifest.blob_filename()).await.unwrap().is_some(),
    );
    assert!(storage.open_hosted_blob(&repository, &layer.blob_filename()).await.unwrap().is_some());
    assert!(
        storage.open_hosted_blob(&nested, &nested_orphan.blob_filename()).await.unwrap().is_none(),
    );
}

#[tokio::test]
async fn corrupt_or_missing_manifests_prevent_deletion() {
    let (temp, storage) = setup();
    let repository = name("acme/app");
    let (manifest, _) = retained_image(&storage, &repository).await;
    let orphan = blob(&storage, &name("aaa"), b"orphan").await;
    let path = temp.path().join("store/acme/app").join(manifest.blob_filename());
    tokio::fs::write(&path, b"corrupt").await.unwrap();
    let error = collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT)
        .await
        .unwrap_err();
    assert!(
        matches!(error, RegistryError::BadRequest { reason } if reason.contains("manifest digest mismatch")),
    );
    assert!(
        storage.open_hosted_blob(&name("aaa"), &orphan.blob_filename()).await.unwrap().is_some(),
    );
    tokio::fs::remove_file(path).await.unwrap();
    let error = collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT)
        .await
        .unwrap_err();
    assert!(
        matches!(error, RegistryError::BadRequest { reason } if reason.contains("retained manifest is missing")),
    );
}

#[tokio::test]
async fn index_keeps_children_removed_from_the_document_and_their_layers() {
    let (_temp, storage) = setup();
    let repository = name("acme/app");
    let (child, layer) = retained_image(&storage, &repository).await;
    let bytes = serde_json::to_vec(&json!({
        "schemaVersion": 2, "mediaType": media_type::OCI_IMAGE_INDEX,
        "manifests": [{"digest": child, "size": 0}],
    }))
    .unwrap();
    let index = blob(&storage, &repository, &bytes).await;
    let mut document = ImageDocument::new(repository.as_str());
    document.insert_manifest(ManifestEntry {
        referrer: None,
        digest: index.clone(),
        size: bytes.len() as u64,
        media_type: media_type::OCI_IMAGE_INDEX.into(),
    });
    storage
        .write_hosted_document_if_current(&repository, &document.to_bytes(), None)
        .await
        .unwrap();
    assert_eq!(
        referenced_blobs(&storage, &repository, MANIFEST_LIMIT).await.unwrap(),
        HashSet::from([index.blob_filename(), child.blob_filename(), layer.blob_filename()]),
    );
    assert_eq!(
        collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (0, 0),
    );
}

#[tokio::test]
async fn flat_registry_collection_excludes_other_namespaces() {
    let (_temp, storage) = setup();
    blob(&storage, &name("other/app"), b"private").await;
    blob(&storage, &name("app"), b"orphan").await;
    assert_eq!(
        collect(&storage, Duration::ZERO, false, &HashSet::from(["other"]), MANIFEST_LIMIT)
            .await
            .unwrap(),
        (1, 6),
    );
}

#[tokio::test]
async fn collection_uses_the_stored_media_type_for_header_only_manifests() {
    let (_temp, storage) = setup();
    let repository = name("app");
    let layer = blob(&storage, &repository, b"layer").await;
    let bytes = serde_json::to_vec(&json!({
        "schemaVersion": 2, "config": {"digest": layer, "size": 5}, "layers": [],
    }))
    .unwrap();
    let digest = blob(&storage, &repository, &bytes).await;
    let mut document = ImageDocument::new(repository.as_str());
    document.insert_manifest(ManifestEntry {
        referrer: None,
        digest: digest.clone(),
        size: bytes.len() as u64,
        media_type: media_type::OCI_IMAGE_MANIFEST.into(),
    });
    storage
        .write_hosted_document_if_current(&repository, &document.to_bytes(), None)
        .await
        .unwrap();
    assert_eq!(
        referenced_blobs(&storage, &repository, MANIFEST_LIMIT).await.unwrap(),
        HashSet::from([digest.blob_filename(), layer.blob_filename()]),
    );
}

#[tokio::test]
async fn a_document_without_any_blob_files_still_blocks_collection_when_corrupt() {
    let (_temp, storage) = setup();
    let repository = name("empty");
    let mut document = ImageDocument::new(repository.as_str());
    document.insert_manifest(ManifestEntry {
        referrer: None,
        digest: Digest::of(b"missing"),
        size: 7,
        media_type: media_type::OCI_IMAGE_MANIFEST.into(),
    });
    storage
        .write_hosted_document_if_current(&repository, &document.to_bytes(), None)
        .await
        .unwrap();
    let orphan = blob(&storage, &name("aaa"), b"orphan").await;
    let error = collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT)
        .await
        .unwrap_err();
    assert!(
        matches!(error, RegistryError::BadRequest { reason } if reason.contains("retained manifest is missing")),
    );
    assert!(
        storage.open_hosted_blob(&name("aaa"), &orphan.blob_filename()).await.unwrap().is_some(),
    );
}

#[tokio::test]
async fn collection_streams_an_object_store_inventory() {
    let temp = TempDir::new().unwrap();
    let config = HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::new(object_store::memory::InMemory::new()),
        prefix: "images/".into(),
    };
    let storage =
        Storage::new(&config, temp.path().join("store"), temp.path().join("cache")).unwrap();
    let repository = name("acme/app");
    retained_image(&storage, &repository).await;
    blob(&storage, &name("acme/app/nested"), b"orphan").await;
    assert_eq!(
        collect(&storage, Duration::ZERO, true, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (1, 6),
    );
    assert_eq!(
        collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (1, 6),
    );
    assert_eq!(
        collect(&storage, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (0, 0),
    );
}

#[tokio::test]
async fn collection_does_not_remove_a_matching_blob_from_the_shared_cache() {
    let (temp, storage) = setup();
    let hosted = storage.for_hosted("images");
    let repository = name("acme/app");
    let digest = blob(&hosted, &repository, b"orphan").await;
    let cache = temp.path().join("cache/acme/app").join(digest.blob_filename());
    tokio::fs::create_dir_all(cache.parent().unwrap()).await.unwrap();
    tokio::fs::write(&cache, b"cached").await.unwrap();
    assert_eq!(
        collect(&hosted, Duration::ZERO, false, &HashSet::new(), MANIFEST_LIMIT).await.unwrap(),
        (1, 6),
    );
    assert_eq!(tokio::fs::read(cache).await.unwrap(), b"cached");
}

#[tokio::test]
async fn offline_collection_finishes_interrupted_explicit_deletion() {
    let (_temp, storage) = setup();
    let repository = name("acme/app");
    let digest = blob(&storage, &repository, b"pending deletion").await;
    let mut document = ImageDocument::new(repository.as_str());
    document.generation = 1;
    document.deleting_blob = Some(digest.clone());
    storage
        .write_hosted_document_if_current(&repository, &document.to_bytes(), None)
        .await
        .unwrap();
    assert_eq!(
        collect(&storage, Duration::from_hours(24), true, &HashSet::new(), MANIFEST_LIMIT)
            .await
            .unwrap(),
        (0, 0),
    );
    assert!(
        storage.open_hosted_blob(&repository, &digest.blob_filename()).await.unwrap().is_some(),
    );
    assert_eq!(
        collect(&storage, Duration::from_hours(24), false, &HashSet::new(), MANIFEST_LIMIT)
            .await
            .unwrap(),
        (1, 16),
    );
    assert!(
        storage.open_hosted_blob(&repository, &digest.blob_filename()).await.unwrap().is_none(),
    );
    let document =
        ImageDocument::parse(&storage.read_hosted_document(&repository).await.unwrap().unwrap())
            .unwrap();
    assert_eq!(document.generation, 1);
    assert!(document.deleting_blob.is_none());
}
