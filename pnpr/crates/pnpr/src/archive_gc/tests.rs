use super::{Collection, liveness};
use crate::RegistryError;
use pnpr_config::HostedStoreConfig;
use pnpr_oci::{Digest, ImageDocument, ManifestEntry, media_type};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::{
    HostedDocumentVersion, Storage, layout::local_blob_path, metadata::ReclaimTarget,
};
use serde_json::json;
use std::{collections::HashSet, time::Duration};
use tempfile::TempDir;

const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;

/// Collect the OCI namespace of `catalog` that `storage` views, finishing
/// its interrupted reclaims first unless it is a dry run, and report how
/// many archives and bytes went.
async fn collect_in(
    storage: &Storage,
    catalog: &str,
    min_age: Duration,
    dry_run: bool,
) -> Result<(usize, u64), RegistryError> {
    let collection = Collection {
        storage,
        catalog,
        ecosystem: Ecosystem::Oci,
        manifest_limit: MANIFEST_LIMIT,
        stale_after: Duration::ZERO,
    };
    if !dry_run {
        collection.finish_pending().await?;
    }
    let report = collection.collect(min_age, dry_run).await?;
    Ok((report.archives, report.bytes))
}

async fn collect(
    storage: &Storage,
    min_age: Duration,
    dry_run: bool,
    _manifest_limit: usize,
) -> Result<(usize, u64), RegistryError> {
    collect_in(storage, "", min_age, dry_run).await
}

async fn referenced_blobs(
    storage: &Storage,
    repository: &CanonicalPackageName,
    manifest_limit: usize,
) -> Result<HashSet<String>, RegistryError> {
    let bytes = storage.read_hosted_document(repository).await?.unwrap();
    let document = liveness::parse_image_document(&bytes)?;
    liveness::referenced_document_blobs(storage, repository, &document, manifest_limit).await
}

fn setup() -> (TempDir, Storage) {
    let temp = TempDir::new().unwrap();
    let storage = Storage::new(
        &HostedStoreConfig::Fs,
        temp.path().join("store"),
        temp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(temp.path().join("metadata.sqlite")),
    )
    .unwrap()
    .for_hosted("", Ecosystem::Oci);
    (temp, storage)
}

fn name(repository: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(repository, Ecosystem::Oci).unwrap()
}

async fn blob(storage: &Storage, repository: &CanonicalPackageName, bytes: &[u8]) -> Digest {
    let digest = Digest::of(bytes);
    let slot = storage
        .reserve_hosted_blob(repository, &digest.blob_filename())
        .await
        .unwrap();
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
    storage
        .write_hosted_document_if_current(repository, &document.to_bytes(), None, None)
        .await
        .unwrap();
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
        collect(&storage, Duration::from_hours(24), false, MANIFEST_LIMIT).await.unwrap(),
        (0, 0),
    );
    assert_eq!(collect(&storage, Duration::ZERO, true, MANIFEST_LIMIT).await.unwrap(), (2, 19));
    assert!(
        storage
            .open_hosted_blob(&nested, &nested_orphan.blob_filename())
            .await
            .unwrap()
            .is_some(),
    );
    assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (2, 19));
    assert!(
        storage
            .open_hosted_blob(&repository, &manifest.blob_filename())
            .await
            .unwrap()
            .is_some(),
    );
    assert!(
        storage
            .open_hosted_blob(&repository, &layer.blob_filename())
            .await
            .unwrap()
            .is_some(),
    );
    assert!(
        storage
            .open_hosted_blob(&nested, &nested_orphan.blob_filename())
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn corrupt_or_missing_manifests_prevent_deletion() {
    let (temp, storage) = setup();
    let repository = name("acme/app");
    let (manifest, _) = retained_image(&storage, &repository).await;
    let orphan = blob(&storage, &name("aaa"), b"orphan").await;
    let path = local_blob_path(
        &temp.path().join("store"),
        "",
        Ecosystem::Oci,
        repository.as_str(),
        &manifest.blob_filename(),
    );
    tokio::fs::write(&path, b"corrupt").await.unwrap();
    let error = collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap_err();
    assert!(
        matches!(error, RegistryError::BadRequest { reason } if reason.contains("manifest digest mismatch")),
    );
    // The repository whose manifest is corrupt keeps every blob; another
    // repository is collected on its own.
    assert!(
        storage
            .open_hosted_blob(&repository, &manifest.blob_filename())
            .await
            .unwrap()
            .is_some(),
    );
    assert!(
        storage
            .open_hosted_blob(&name("aaa"), &orphan.blob_filename())
            .await
            .unwrap()
            .is_none(),
    );
    tokio::fs::remove_file(path).await.unwrap();
    let error = collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap_err();
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
        .write_hosted_document_if_current(
            &repository,
            &document.to_bytes(),
            Some(&HostedDocumentVersion::Unversioned),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        referenced_blobs(&storage, &repository, MANIFEST_LIMIT).await.unwrap(),
        HashSet::from([index.blob_filename(), child.blob_filename(), layer.blob_filename()]),
    );
    assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (0, 0));
}

#[tokio::test]
async fn collection_reaches_only_its_own_namespace() {
    let (_temp, storage) = setup();
    let org = storage.for_hosted("other", Ecosystem::Oci);
    blob(&org, &name("app"), b"private").await;
    blob(&storage, &name("app"), b"orphan").await;
    assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (1, 6));
    assert_eq!(collect(&org, Duration::ZERO, true, MANIFEST_LIMIT).await.unwrap(), (1, 7));
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
        .write_hosted_document_if_current(&repository, &document.to_bytes(), None, None)
        .await
        .unwrap();
    assert_eq!(
        referenced_blobs(&storage, &repository, MANIFEST_LIMIT).await.unwrap(),
        HashSet::from([digest.blob_filename(), layer.blob_filename()]),
    );
}

#[tokio::test]
async fn a_corrupt_repository_without_blobs_leaves_other_repositories_collectable() {
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
        .write_hosted_document_if_current(&repository, &document.to_bytes(), None, None)
        .await
        .unwrap();
    // A repository's manifests reach only its own blobs, so one with no
    // blobs has nothing a corrupt document could keep.
    let orphan = blob(&storage, &name("aaa"), b"orphan").await;
    assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (1, 6));
    assert!(
        storage
            .open_hosted_blob(&name("aaa"), &orphan.blob_filename())
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn collection_streams_an_object_store_inventory() {
    let temp = TempDir::new().unwrap();
    let config = HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::new(object_store::memory::InMemory::new()),
        prefix: "images/".into(),
    };
    let storage = Storage::new(
        &config,
        temp.path().join("store"),
        temp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(temp.path().join("metadata.sqlite")),
    )
    .unwrap()
    .for_hosted("", Ecosystem::Oci);
    let repository = name("acme/app");
    retained_image(&storage, &repository).await;
    blob(&storage, &name("acme/app/nested"), b"orphan").await;
    assert_eq!(collect(&storage, Duration::ZERO, true, MANIFEST_LIMIT).await.unwrap(), (1, 6));
    assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (1, 6));
    assert_eq!(collect(&storage, Duration::ZERO, false, MANIFEST_LIMIT).await.unwrap(), (0, 0));
}

#[tokio::test]
async fn collection_does_not_remove_a_matching_blob_from_the_shared_cache() {
    let (temp, storage) = setup();
    let hosted = storage.for_hosted("images", Ecosystem::Oci);
    let repository = name("acme/app");
    let digest = blob(&hosted, &repository, b"orphan").await;
    let cache = temp
        .path()
        .join("cache/acme/app")
        .join(digest.blob_filename());
    tokio::fs::create_dir_all(cache.parent().unwrap()).await.unwrap();
    tokio::fs::write(&cache, b"cached").await.unwrap();
    assert_eq!(collect_in(&hosted, "images", Duration::ZERO, false).await.unwrap(), (1, 6));
    assert_eq!(tokio::fs::read(cache).await.unwrap(), b"cached");
}

/// A reclaim that stopped after marking its blob is finished by the next
/// collection, whatever the blob's age, and no longer blocks a commit.
#[tokio::test]
async fn collection_finishes_an_interrupted_reclaim() {
    let (_temp, storage) = setup();
    let repository = name("acme/app");
    let digest = blob(&storage, &repository, b"pending deletion").await;
    let target = ReclaimTarget {
        ecosystem: Ecosystem::Oci,
        name: repository.as_str().to_string(),
        expected: None,
        artifacts: vec![digest.blob_filename()],
    };
    let database = storage.sql_metadata();
    let owner = pnpr_storage::metadata::new_reclaim_owner();
    assert_eq!(
        database
            .mark_reclaims("", &owner, vec![target])
            .await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        collect(&storage, Duration::from_hours(24), true, MANIFEST_LIMIT).await.unwrap(),
        (0, 0),
    );
    assert!(
        storage
            .open_hosted_blob(&repository, &digest.blob_filename())
            .await
            .unwrap()
            .is_some(),
    );
    assert_eq!(
        collect(&storage, Duration::from_hours(24), false, MANIFEST_LIMIT).await.unwrap(),
        (0, 0),
    );
    assert!(
        storage
            .open_hosted_blob(&repository, &digest.blob_filename())
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        database
            .pending_reclaims("")
            .await
            .unwrap()
            .is_empty(),
    );
}

/// A publish refused at its commit reclaims only what its package does not
/// reference by then: another replica may have found an archive it wrote in
/// place and committed it.
#[tokio::test]
async fn a_refused_publish_keeps_what_its_package_references_by_then() {
    let temp = TempDir::new().unwrap();
    let root = Storage::new(
        &HostedStoreConfig::Fs,
        temp.path().join("store"),
        temp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(temp.path().join("metadata.sqlite")),
    )
    .unwrap();
    let storage = root.for_hosted("", Ecosystem::Npm);
    let package = CanonicalPackageName::parse("@acme/widget", Ecosystem::Npm).unwrap();
    for file in ["widget-1.0.0.tgz", "widget-2.0.0.tgz", "notes.txt"] {
        let slot = storage.reserve_hosted_blob(&package, file).await.unwrap();
        tokio::fs::write(&slot.tmp_path, file).await.unwrap();
        storage.finalize_blob_slot(slot).await.unwrap();
    }
    let document = json!({
        "name": "@acme/widget",
        "versions": { "1.0.0": { "name": "@acme/widget", "version": "1.0.0" } },
    });
    storage
        .write_hosted_document_if_current(
            &package,
            &serde_json::to_vec(&document).unwrap(),
            None,
            None,
        )
        .await
        .unwrap();

    let written = ["widget-1.0.0.tgz", "widget-2.0.0.tgz", "notes.txt"].map(str::to_string);
    let removed =
        super::reclaim::reclaim_unreferenced(&root, "", &package, written.to_vec(), MANIFEST_LIMIT)
            .await
            .unwrap();
    assert_eq!(
        removed
            .iter()
            .map(|archive| archive.artifact.as_str())
            .collect::<Vec<_>>(),
        ["widget-2.0.0.tgz"],
    );
    let mut left = storage.hosted_package_blobs(&package).await.unwrap();
    left.sort();
    assert_eq!(left, ["notes.txt", "widget-1.0.0.tgz"]);
}
