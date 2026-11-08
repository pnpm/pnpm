//! The blob fence on the commit path: a publish that depends on bytes a
//! reclaim took after the publish read them is refused, and a refused
//! publish discards the incarnation it stored.

use crate::server::{
    AppState, oci::OciPublication, publishing::commit_publishes, routing::state_and_router,
};
use axum::body::Bytes;
use pnpr_auth::AuthState;
use pnpr_error::RegistryError;
use pnpr_fixtures::MockConfigPresets;
use pnpr_oci::{Digest, media_type};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::Storage;
use serde_json::json;
use tempfile::TempDir;

fn state(temp: &TempDir) -> AppState {
    let config =
        crate::Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().join("store"));
    state_and_router(config, AuthState::in_memory(), None).unwrap().0
}

fn images(state: &AppState) -> Storage {
    state.inner.storage.for_hosted("", Ecosystem::Oci)
}

/// Upload `bytes` as an image blob of `repository`, as a finished upload
/// maps it.
async fn put_blob(storage: &Storage, repository: &CanonicalPackageName, bytes: &[u8]) -> Digest {
    let digest = Digest::of(bytes);
    let slot = storage
        .reserve_hosted_blob(repository, &digest.blob_filename())
        .await
        .unwrap();
    tokio::fs::write(&slot.tmp_path, bytes).await.unwrap();
    let (id, size) = storage.identify_blob_slot(&slot).await.unwrap();
    let rows = storage
        .blob_rows(vec![id.clone()])
        .await
        .unwrap();
    let promoted = storage
        .promote_blob_slot(slot, (id.clone(), size), rows.get(&id))
        .await
        .unwrap();
    storage.map_hosted_artifact(promoted.artifact).await.unwrap();
    digest
}

/// The keys the namespace of `storage` stores archives under.
async fn stored_keys(storage: &Storage) -> Vec<String> {
    let mut keys: Vec<String> = storage
        .stored_incarnations()
        .await
        .unwrap()
        .into_iter()
        .map(|(blob, _)| blob.key())
        .collect();
    keys.sort();
    keys
}

async fn has_blob(storage: &Storage, repository: &CanonicalPackageName, digest: &Digest) -> bool {
    storage
        .open_hosted_blob(repository, &digest.blob_filename())
        .await
        .unwrap()
        .is_some()
}

fn manifest(layer: &Digest) -> Bytes {
    Bytes::from(
        serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": media_type::OCI_IMAGE_MANIFEST,
            "config": { "mediaType": "application/example.config", "digest": layer, "size": 5 },
            "layers": [],
        }))
        .unwrap(),
    )
}

fn push(repository: &CanonicalPackageName, bytes: Bytes) -> OciPublication {
    OciPublication::new(
        (repository.clone(), String::new()),
        "latest".to_string(),
        bytes,
        Some(media_type::OCI_IMAGE_MANIFEST),
        4 * 1024 * 1024,
    )
    .map_err(|_| "a valid manifest")
    .unwrap()
}

/// A manifest push read its layer live, a reclaim took the layer once nothing
/// mapped it, and the push then commits: it is refused, and the manifest
/// incarnation it stored goes with it. A push after the reclaim finds the
/// layer missing, and one after the layer is uploaded again, as its next
/// incarnation, commits.
#[tokio::test]
async fn a_manifest_whose_layer_was_reclaimed_after_it_was_checked_is_refused() {
    let temp = TempDir::new().unwrap();
    let state = state(&temp);
    let storage = images(&state);
    let repository = CanonicalPackageName::parse("acme/app", Ecosystem::Oci).unwrap();
    let layer = put_blob(&storage, &repository, b"layer").await;
    let bytes = manifest(&layer);
    let staged = push(&repository, bytes.clone())
        .stage(&state)
        .await
        .map_err(|_| "staged")
        .unwrap();

    let artifacts = vec![layer.blob_filename()];
    let removed = storage.unmap_hosted_artifacts(&repository, None, artifacts).await.unwrap();
    assert_eq!(removed.map(|removed| removed.len()), Some(1));
    assert!(!has_blob(&storage, &repository, &layer).await);
    assert!(stored_keys(&storage).await.is_empty());

    let err = commit_publishes(&state, vec![staged]).await.unwrap_err();
    assert!(matches!(err, RegistryError::ArchiveReclaimed { .. }), "{err:?}");
    assert!(
        storage
            .read_hosted_document(&repository)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(!has_blob(&storage, &repository, &Digest::of(&bytes)).await);
    assert!(stored_keys(&storage).await.is_empty(), "the refused manifest is discarded");

    assert!(push(&repository, bytes.clone()).stage(&state).await.is_err());
    put_blob(&storage, &repository, b"layer").await;
    let layer_id = pnpr_storage::BlobId::parse(&layer.blob_filename()).unwrap();
    assert_eq!(
        storage
            .hosted_blob_of(&repository, &layer.blob_filename())
            .await
            .unwrap(),
        Some(pnpr_storage::metadata::BlobRef { id: layer_id, incarnation: 2 }),
    );
    let staged = push(&repository, bytes.clone())
        .stage(&state)
        .await
        .map_err(|_| "staged")
        .unwrap();
    commit_publishes(&state, vec![staged]).await.unwrap();
    assert!(
        storage
            .read_hosted_document(&repository)
            .await
            .unwrap()
            .is_some(),
    );
    assert!(has_blob(&storage, &repository, &layer).await);
}

/// A manifest push read its layer, which another repository maps too, and
/// the layer was then deleted from the push's repository: the push is
/// refused rather than mapping the deleted layer back, and the other
/// repository keeps the bytes.
#[tokio::test]
async fn a_manifest_does_not_bring_back_a_layer_deleted_from_its_repository() {
    let temp = TempDir::new().unwrap();
    let state = state(&temp);
    let storage = images(&state);
    let repository = CanonicalPackageName::parse("acme/app", Ecosystem::Oci).unwrap();
    let other = CanonicalPackageName::parse("acme/other", Ecosystem::Oci).unwrap();
    let layer = put_blob(&storage, &repository, b"layer").await;
    put_blob(&storage, &other, b"layer").await;
    let staged = push(&repository, manifest(&layer))
        .stage(&state)
        .await
        .map_err(|_| "staged")
        .unwrap();

    let artifacts = vec![layer.blob_filename()];
    let removed = storage.unmap_hosted_artifacts(&repository, None, artifacts).await.unwrap();
    assert_eq!(removed.map(|removed| removed.len()), Some(0), "the other repository keeps it");

    let err = commit_publishes(&state, vec![staged]).await.unwrap_err();
    assert!(matches!(err, RegistryError::ArchiveReclaimed { .. }), "{err:?}");
    assert!(!has_blob(&storage, &repository, &layer).await);
    assert!(has_blob(&storage, &other, &layer).await);
}
