//! The reclaim fence on the commit path: a publish whose archives a reclaim
//! removed after the publish read its package is refused, and a refused
//! publish reclaims what it wrote.

use crate::{
    archive_gc::reclaim::reclaim,
    server::{
        AppState, oci::OciPublication, publishing::commit_publishes, routing::state_and_router,
    },
};
use axum::body::Bytes;
use pnpr_auth::AuthState;
use pnpr_error::RegistryError;
use pnpr_fixtures::MockConfigPresets;
use pnpr_oci::{Digest, media_type};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::{Storage, metadata::ReclaimTarget};
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

async fn put_blob(storage: &Storage, repository: &CanonicalPackageName, bytes: &[u8]) -> Digest {
    let digest = Digest::of(bytes);
    let slot = storage
        .reserve_hosted_blob(repository, &digest.blob_filename())
        .await
        .unwrap();
    tokio::fs::write(&slot.tmp_path, bytes).await.unwrap();
    storage.finalize_blob_slot(slot).await.unwrap();
    digest
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

/// A manifest push checked its layer, a reclaim removed the layer, and the
/// push then commits: it is refused, and the manifest blob it wrote goes
/// with it. A push after the reclaim finds the layer missing, and one after
/// the layer is uploaded again commits.
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

    let target = ReclaimTarget {
        ecosystem: Ecosystem::Oci,
        name: repository.as_str().to_string(),
        expected: None,
        artifacts: vec![layer.blob_filename()],
    };
    let removed = reclaim(&state.inner.storage, "", vec![target]).await.unwrap();
    assert_eq!(removed.len(), 1);
    assert!(!has_blob(&storage, &repository, &layer).await);

    let err = commit_publishes(&state, vec![staged]).await.unwrap_err();
    assert!(matches!(err, RegistryError::ArchiveReclaimed { .. }), "{err:?}");
    assert!(
        storage
            .read_hosted_document(&repository)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!has_blob(&storage, &repository, &Digest::of(&bytes)).await);

    assert!(push(&repository, bytes.clone()).stage(&state).await.is_err());
    put_blob(&storage, &repository, b"layer").await;
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
            .is_some()
    );
    assert!(has_blob(&storage, &repository, &layer).await);
}
