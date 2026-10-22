use super::{
    ArtifactBlobRequest, BASE64, HostedStoreConfig, OwnerScope, Sha512, SharedArtifactStore,
    TempDir, lookup, publication_with_blob,
};
use crate::{blob_id, org_key, staged_record_path};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _, stream};
use object_store::ObjectStoreExt as _;
use sha2::Digest as _;
use std::convert::Infallible;

fn body(bytes: &[u8]) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin + use<> {
    let chunks: Vec<_> = bytes
        .chunks(64 * 1024)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    stream::iter(chunks)
}

fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)))
}

fn acme() -> OwnerScope {
    OwnerScope::organization("acme")
}

async fn global_bytes(store: &SharedArtifactStore) -> u64 {
    store.load_usage().await.unwrap().0.global_bytes
}

async fn read_back(store: &SharedArtifactStore, integrity: &str) -> Vec<u8> {
    let request = ArtifactBlobRequest { owner: acme(), integrity: integrity.to_string() };
    let mut blob = store
        .read_blob("acme", &serde_json::to_vec(&request).unwrap())
        .await
        .unwrap()
        .expect("stored blob");
    let mut bytes = Vec::new();
    while let Some(chunk) = blob.stream.next().await {
        bytes.extend_from_slice(&chunk.unwrap());
    }
    bytes
}

#[tokio::test]
async fn a_blob_uploaded_alone_completes_a_publication_that_carries_none() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let mut request =
        publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/blob-upload");
    let upload = request.blobs.remove(0);
    let bytes = BASE64.decode(&upload.data).unwrap();
    let size = bytes.len() as u64;

    let owner = acme();
    let stored = store.store_blob("acme", &owner, &upload.integrity, size, body(&bytes));
    assert!(stored.await.unwrap(), "the first upload stores the blob");
    assert_eq!(
        store
            .blob_size("acme", &acme(), &upload.integrity)
            .await
            .unwrap(),
        Some(size),
    );
    assert_eq!(global_bytes(&store).await, size);
    let again = store.store_blob("acme", &owner, &upload.integrity, size, body(&bytes));
    assert!(!again.await.unwrap(), "an upload of a stored blob stores nothing");
    assert_eq!(global_bytes(&store).await, size, "a stored blob is charged once");

    assert!(store.publish("acme", request).await.unwrap());
    let response = store
        .resolve("acme", &serde_json::to_vec(&lookup("acme")).unwrap())
        .await
        .unwrap();
    assert_eq!(response.artifacts.len(), 1);
    assert_eq!(read_back(&store, &upload.integrity).await, bytes);
}

#[tokio::test]
async fn an_upload_that_does_not_match_its_integrity_is_not_stored() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let declared = integrity(b"shared addon");
    for (sent, size) in [
        (&b"altered addon"[..], 12),
        (&b"shared addon and more"[..], 12),
        (&b"shared"[..], 12),
        (&b"altered addon"[..], 13),
    ] {
        let error = store
            .store_blob("acme", &acme(), &declared, size, body(sent))
            .await
            .expect_err("a mismatched upload is refused");
        eprintln!("{error}");
    }
    assert_eq!(
        store
            .blob_size("acme", &acme(), &declared)
            .await
            .unwrap(),
        None,
    );
    assert_eq!(global_bytes(&store).await, 0, "a refused upload gives its reservation back");
}

#[tokio::test]
async fn a_blob_larger_than_one_write_is_streamed_in_parts() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let bytes: Vec<u8> = (0..9 * 1024 * 1024_u32)
        .map(|index| (index % 251) as u8)
        .collect();
    let size = bytes.len() as u64;
    let declared = integrity(&bytes);

    let mut altered = bytes.clone();
    altered[size as usize / 2] ^= 1;
    store
        .store_blob("acme", &acme(), &declared, size, body(&altered))
        .await
        .expect_err("a large mismatched upload is refused after its last byte");
    assert_eq!(
        store
            .blob_size("acme", &acme(), &declared)
            .await
            .unwrap(),
        None,
    );

    assert!(
        store
            .store_blob("acme", &acme(), &declared, size, body(&bytes))
            .await
            .unwrap(),
    );
    assert_eq!(read_back(&store, &declared).await, bytes);
}

#[tokio::test]
async fn an_uploaded_blob_waits_for_its_publication_through_reclamation() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let declared = integrity(b"shared addon");
    store
        .store_blob("acme", &acme(), &declared, 12, body(b"shared addon"))
        .await
        .unwrap();
    let reclaim = async || {
        store
            .mutate_usage(|usage| {
                usage.reclamation_needed = true;
                Ok(true)
            })
            .await
            .unwrap();
        store.try_reclaim_unreferenced_blobs().await.unwrap();
    };

    reclaim().await;
    assert_eq!(
        store
            .blob_size("acme", &acme(), &declared)
            .await
            .unwrap(),
        Some(12),
    );

    let record = staged_record_path(&org_key("acme"), &blob_id(&declared).unwrap());
    store.store
        .delete(&store.object_path(&record))
        .await
        .unwrap();
    reclaim().await;
    assert_eq!(
        store
            .blob_size("acme", &acme(), &declared)
            .await
            .unwrap(),
        None,
        "an unreferenced blob no upload vouches for is reclaimed",
    );
}

#[tokio::test]
async fn only_a_publisher_may_upload_a_blob() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let declared = integrity(b"shared addon");
    store
        .store_blob("someone-else", &acme(), &declared, 12, body(b"shared addon"))
        .await
        .expect_err("only a publisher of acme may upload");
    assert_eq!(
        store
            .blob_size("acme", &acme(), &declared)
            .await
            .unwrap(),
        None,
    );
}
