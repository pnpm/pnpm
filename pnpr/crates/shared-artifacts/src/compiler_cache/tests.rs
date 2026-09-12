use bytes::Bytes;
use futures_util::future::join_all;
use object_store::{ObjectStoreExt as _, memory::InMemory};
use pnpr_config::HostedStoreConfig;
use std::sync::Arc;
use tempfile::TempDir;

use super::{CompilerCacheKey, DIGEST_SIZE, compiler_cache_path};
use crate::{SharedArtifactStore, owner_key};
use pnpm_shared_artifact_protocol::OwnerScope;

fn key(value: &str) -> CompilerCacheKey {
    CompilerCacheKey::try_from(value.to_string()).unwrap()
}

#[test]
fn keys_reject_traversal_and_ambiguous_paths() {
    for invalid in ["", "/abc", "a//b", "a/../b", "./abc", r"a\b", "a/%2e", "a?b", "a#b"] {
        let result = CompilerCacheKey::try_from(invalid.to_string());
        assert!(result.is_err(), "accepted {invalid:?}");
    }
    assert!(CompilerCacheKey::try_from("a".repeat(1025)).is_err(), "accepted oversized key");
    key("rust-v1/a/b/123abc");
}

#[tokio::test]
async fn local_entries_survive_restart_and_are_isolated_by_owner_and_key() {
    let directory = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, directory.path()).unwrap();
    let input = key("a/b/cache-key");
    assert_eq!(store.read_compiler_cache("ci", &input).await.unwrap(), None);
    assert!(
        store.publish_compiler_cache("ci", &input, Bytes::from_static(b"compiled")).await.unwrap(),
        "first publication must create the entry",
    );
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, directory.path()).unwrap();
    assert_eq!(store.read_compiler_cache("ci", &input).await.unwrap().unwrap(), "compiled");
    assert_eq!(store.read_compiler_cache("other", &input).await.unwrap(), None);
    assert_eq!(store.read_compiler_cache("ci", &key("different")).await.unwrap(), None);
    assert!(
        !store
            .publish_compiler_cache("ci", &input, Bytes::from_static(b"replacement"))
            .await
            .unwrap(),
        "a later publication must not replace the entry",
    );
    assert_eq!(store.read_compiler_cache("ci", &input).await.unwrap().unwrap(), "compiled");
}

#[tokio::test]
async fn replicas_publish_one_immutable_entry_and_charge_quota_once() {
    let directory = TempDir::new().unwrap();
    let backend = HostedStoreConfig::ObjectStore {
        store: Arc::new(InMemory::new()),
        prefix: "test/".to_string(),
    };
    let replicas = (0..8)
        .map(|_| SharedArtifactStore::new(&backend, directory.path()).unwrap())
        .collect::<Vec<_>>();
    let input = key("cache-key");
    let results = join_all(replicas.iter().enumerate().map(|(index, store)| {
        let input = &input;
        async move {
            store.publish_compiler_cache("ci", input, Bytes::from(vec![index as u8])).await.unwrap()
        }
    }))
    .await;
    assert_eq!(results.iter().filter(|created| **created).count(), 1);
    let usage = replicas[0].load_usage().await.unwrap().0;
    assert_eq!(usage.global_bytes, (DIGEST_SIZE + 1) as u64);
    assert_eq!(usage.active_publications.len(), 0);
    let winner = results.iter().position(|created| *created).unwrap() as u8;
    assert_eq!(replicas[1].read_compiler_cache("ci", &input).await.unwrap().unwrap()[0], winner);
}

#[tokio::test]
async fn quota_rejects_new_entries_but_allows_hits_and_retries() {
    let directory = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, directory.path())
        .unwrap()
        .with_limits((DIGEST_SIZE + 1) as u64, 1024);
    let input = key("one");
    store.publish_compiler_cache("ci", &input, Bytes::from_static(b"a")).await.unwrap();
    assert!(
        !store.publish_compiler_cache("ci", &input, Bytes::from_static(b"b")).await.unwrap(),
        "a retry must work at the quota limit",
    );
    let rejected = store.publish_compiler_cache("ci", &key("two"), Bytes::from_static(b"b")).await;
    assert!(rejected.is_err(), "quota accepted a second entry: {rejected:?}");
    assert_eq!(store.read_compiler_cache("ci", &input).await.unwrap().unwrap(), "a");
    assert_eq!(store.load_usage().await.unwrap().0.global_bytes, (DIGEST_SIZE + 1) as u64);
}

#[tokio::test]
async fn corrupted_and_relocated_entries_are_never_served() {
    let directory = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, directory.path()).unwrap();
    let input = key("original");
    store.publish_compiler_cache("ci", &input, Bytes::from_static(b"compiled")).await.unwrap();
    let owner = owner_key("ci", &OwnerScope::organization("ci")).unwrap();
    let original = store.object_path(&compiler_cache_path(&owner, &input));
    let bytes = store.store.get(&original).await.unwrap().bytes().await.unwrap();
    let relocated = store.object_path(&compiler_cache_path(&owner, &key("relocated")));
    store.store.put(&relocated, bytes.into()).await.unwrap();
    let result = store.read_compiler_cache("ci", &key("relocated")).await;
    assert!(result.is_err(), "served relocated entry: {result:?}");
    store.store.put(&original, Bytes::from_static(b"truncated").into()).await.unwrap();
    let result = store.read_compiler_cache("ci", &input).await;
    assert!(result.is_err(), "served corrupted entry: {result:?}");
}

#[tokio::test]
async fn metadata_and_duplicate_publication_do_not_read_or_verify_payloads() {
    let directory = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, directory.path()).unwrap();
    let input = key("metadata");
    assert_eq!(store.compiler_cache_size("ci", &input).await.unwrap(), None);
    store.publish_compiler_cache("ci", &input, Bytes::from_static(b"compiled")).await.unwrap();
    assert_eq!(store.compiler_cache_size("ci", &input).await.unwrap(), Some(8));
    let owner = owner_key("ci", &OwnerScope::organization("ci")).unwrap();
    let path = store.object_path(&compiler_cache_path(&owner, &input));
    store.store.put(&path, vec![0; DIGEST_SIZE + 8].into()).await.unwrap();
    assert_eq!(store.compiler_cache_size("ci", &input).await.unwrap(), Some(8));
    assert!(
        !store
            .publish_compiler_cache("ci", &input, Bytes::from_static(b"replacement"))
            .await
            .unwrap(),
        "metadata-only duplicate check must preserve the immutable entry",
    );
    assert!(
        store.read_compiler_cache("ci", &input).await.is_err(),
        "GET must still reject corruption",
    );
    store.store.put(&path, Bytes::from_static(b"truncated").into()).await.unwrap();
    assert!(
        store.compiler_cache_size("ci", &input).await.is_err(),
        "invalid stored size must fail",
    );
}
