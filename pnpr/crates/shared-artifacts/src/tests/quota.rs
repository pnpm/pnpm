use super::{
    Arc, ArtifactUsage, AtomicBool, AtomicUsize, CompilerCacheKey, FailArtifactWrites,
    HostedStoreConfig, InMemory, ObjectPath, ObjectStore, ObjectStoreExt, Ordering,
    SharedArtifactStore, TempDir, artifact_operation_id, for_platform, is_write_conflict, lookup,
    normalize_key_prefix, publication, publication_for_platform, publication_tagged,
    publication_with_blob,
};

#[tokio::test]
async fn compiler_cache_survives_side_effects_reclamation_and_shares_quota() {
    let directory = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, directory.path()).unwrap();
    let key = CompilerCacheKey::try_from("rust/cache-key".to_string()).unwrap();
    store
        .publish_compiler_cache("acme", &key, bytes::Bytes::from_static(b"compiled"))
        .await
        .unwrap();
    store
        .publish("acme", publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/linux"))
        .await
        .unwrap();
    let before = store.load_usage().await.unwrap().0;
    let after = store.reclaim_unreferenced_blobs().await.unwrap();
    assert_eq!(after.global_bytes, before.global_bytes);
    assert_eq!(after.owner_bytes.len(), 1);
    assert_eq!(store.read_compiler_cache("acme", &key).await.unwrap().unwrap(), "compiled");
}

#[tokio::test]
async fn compiler_cache_failed_writes_reconcile_quota_even_after_remote_commit() {
    for commit_before_error in [false, true] {
        let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
            inner: InMemory::new(),
            commit_before_error,
            fail_deletes: false,
            fail_next_quota_write: None,
            claim_slot_first: None,
            fail_slot_read_after_first: None,
            publish_overlapping_after_create: None,
            fail_reads_of: None,
            fail_scope_writes: false,
            fail_only: None,
            usage_writes: None,
        });
        let hosted = HostedStoreConfig::ObjectStore { store: backend, prefix: String::new() };
        let directory = TempDir::new().unwrap();
        let store = SharedArtifactStore::new(&hosted, directory.path()).unwrap();
        let key = CompilerCacheKey::try_from("cache-key".to_string()).unwrap();
        let result =
            store.publish_compiler_cache("acme", &key, bytes::Bytes::from_static(b"a")).await;
        assert!(result.is_err(), "write failure must surface: {result:?}");
        let usage = store.load_usage().await.unwrap().0;
        assert_eq!(usage.global_bytes, if commit_before_error { 65 } else { 0 });
        assert_eq!(usage.active_publications.len(), 0);
        assert_eq!(
            store.read_compiler_cache("acme", &key).await.unwrap().is_some(),
            commit_before_error,
        );
    }
}

#[test]
fn missing_quota_object_writes_are_not_conflicts() {
    let error = object_store::Error::NotFound {
        path: "quota.json".to_string(),
        source: std::io::Error::other("missing quota object").into(),
    };

    assert!(!is_write_conflict(&error));
}

#[test]
fn quota_state_from_before_reclamation_coordination_remains_readable() {
    let usage: ArtifactUsage =
        serde_json::from_str(r#"{"global_bytes":12,"owner_bytes":{"owner":12}}"#).unwrap();

    assert_eq!(usage.global_bytes, 12);
    assert!(usage.active_publications.is_empty());
    assert!(!usage.reclamation_needed);
    assert!(usage.reclamation.is_none());
}

#[tokio::test]
async fn object_store_replicas_share_blobs_envelopes_and_quota() {
    let backend: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let config = HostedStoreConfig::ObjectStore {
        store: Arc::clone(&backend),
        prefix: "packages".to_string(),
    };
    let first = SharedArtifactStore::new(&config, TempDir::new().unwrap().path()).unwrap();
    let second = SharedArtifactStore::new(&config, TempDir::new().unwrap().path()).unwrap();

    assert!(
        first
            .publish(
                "acme",
                for_platform(
                    publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/first"),
                    1,
                ),
            )
            .await
            .unwrap(),
    );
    assert!(
        second
            .publish(
                "acme",
                for_platform(
                    publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/second"),
                    2,
                ),
            )
            .await
            .unwrap(),
    );

    let response =
        first.resolve("acme", &serde_json::to_vec(&lookup("acme")).unwrap()).await.unwrap();
    assert_eq!(response.artifacts[0].variants.len(), 2);
    let usage_path = object_store::path::Path::from(format!(
        "{}.pnpr-artifacts/v0/quota.json",
        normalize_key_prefix(Some("packages")),
    ));
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.owner_bytes.len(), 1);
    assert!(usage.global_bytes > b"shared addon".len() as u64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_replicas_update_quota_without_lost_writes() {
    const PUBLICATIONS: usize = 16;

    let backend: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let mut publications = Vec::with_capacity(PUBLICATIONS);
    for index in 0..PUBLICATIONS {
        let config =
            HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
        publications.push(tokio::spawn(async move {
            let scratch = TempDir::new().unwrap();
            let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
            store.publish("acme", publication_for_platform(index)).await
        }));
    }
    for publication in publications {
        assert!(publication.await.unwrap().unwrap());
    }

    let usage_path = object_store::path::Path::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    // Each publication stores its envelope and the marker for the one scope it
    // reaches, which the store keeps until reclamation.
    let expected = (0..PUBLICATIONS)
        .map(|index| {
            let publication = publication_for_platform(index);
            serde_json::to_vec(&publication.envelope).unwrap().len()
                + publication.envelope.digest().unwrap().len()
        })
        .sum::<usize>() as u64;
    assert_eq!(usage.global_bytes, expected);
}

#[tokio::test]
async fn quota_is_reserved_before_objects_are_written() {
    let storage = TempDir::new().unwrap();
    let store =
        SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap().with_limits(1, 1);

    let error = store.publish("acme", publication("ci/too-large")).await.unwrap_err();

    assert!(error.to_string().contains("quota exceeded"), "{error}");
    let entries = std::fs::read_dir(storage.path().join("shared-artifacts/v0"))
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_name() != ".locks")
        .collect::<Vec<_>>();
    assert!(entries.is_empty(), "quota rejection wrote objects: {entries:?}");
}

#[tokio::test]
async fn failed_object_writes_reconcile_quota_to_physical_storage() {
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: false,
        fail_deletes: false,
        fail_next_quota_write: None,
        claim_slot_first: None,
        fail_slot_read_after_first: None,
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: None,
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    store.publish("acme", publication("ci/failure")).await.unwrap_err();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert_eq!(usage.owner_bytes.values().copied().sum::<u64>(), 0);
}

#[tokio::test]
async fn publication_finish_retries_a_transient_quota_write_failure() {
    let fail_next_quota_write = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: false,
        fail_deletes: false,
        fail_next_quota_write: Some(Arc::clone(&fail_next_quota_write)),
        claim_slot_first: None,
        fail_slot_read_after_first: None,
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: None,
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let publication = artifact_operation_id().unwrap();
    store.begin_publication(&publication).await.unwrap();
    fail_next_quota_write.store(true, Ordering::SeqCst);

    store.finish_publication(&publication, true).await.unwrap();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert!(usage.active_publications.is_empty());
    assert!(usage.reclamation_needed);
}

/// Losing the race and then failing to read the winner must not leave the loser
/// charged for an envelope it did not store: that debt never comes back, and
/// enough of it starts refusing publications that fit.
#[tokio::test]
async fn a_failed_reread_after_a_lost_race_still_releases_the_quota() {
    let winner = publication("ci/winner");
    let (payload, _) = winner.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&winner.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: true,
        fail_deletes: false,
        fail_next_quota_write: None,
        claim_slot_first: Some((
            format!(".pnpr-artifacts/v0/{owner}/entries/{entry}/{slot}.json"),
            serde_json::to_vec(&winner.envelope).unwrap(),
        )),
        fail_slot_read_after_first: Some(Arc::new(AtomicUsize::new(0))),
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: None,
        usage_writes: None,
    });
    let store = SharedArtifactStore::new(
        &HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() },
        TempDir::new().unwrap().path(),
    )
    .unwrap();

    store.publish("acme", publication("ci/loser")).await.unwrap_err();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    // The winner is written behind the store's back to stage the race, so it is
    // never charged, and the loser's envelope never landed. What the loser did
    // write is the marker claiming the scope it reaches, which the reservation
    // does not carry through a failure this early — the usage scan reclamation
    // ends with picks it up, along with dropping the marker itself.
    assert_eq!(usage.global_bytes, 0, "the loser is not charged for what it did not store");
}

/// A retry of a publication that finished writes nothing, so charging it for
/// what it will not store would refuse one an owner at their limit is entitled
/// to make.
#[tokio::test]
async fn a_retry_of_a_stored_artifact_needs_no_quota() {
    let storage = TempDir::new().unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap());

    let full =
        SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap().with_limits(1, 1);

    assert!(
        !full.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap(),
        "the artifact is already published, and republishing it stores nothing",
    );
}

/// Publishing into an entry that still needs its markers writes some for
/// artifacts somebody else stored. Those are reserved and kept where they are
/// written, so the publication's own accounting neither pays for them nor comes
/// up short releasing what it did not use.
#[tokio::test]
async fn publishing_into_an_entry_that_needs_markers_keeps_its_quota_straight() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let stored = publication_tagged("ci/stored", &["pnpm:v1:linux-arm64-node22-glibc2.17"]);
    let (payload, _) = stored.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&stored.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    store
        .create_object(
            &format!("{owner}/entries/{entry}/{slot}.json"),
            serde_json::to_vec(&stored.envelope).unwrap(),
        )
        .await
        .unwrap();

    // Reaches machines the stored one does not, so it is published rather than
    // refused, and its release runs with the backfill's markers already written.
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    assert!(store.publish("acme", ours).await.unwrap());

    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-arm64-node22"), 128)
            .await
            .unwrap()
            .is_some(),
        "the artifact already there keeps the machines it reaches",
    );
}
