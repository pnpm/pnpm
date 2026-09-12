use super::{
    Arc, ArtifactUsage, Duration, FailArtifactWrites, HostedStoreConfig, InMemory, ObjectPath,
    ObjectStore, ObjectStoreExt, OwnerScope, PutPayload, RegistryError, SharedArtifactStore,
    TempDir, artifact_operation_id, owner_key, publication_tagged, publication_with_blob,
};

#[tokio::test]
async fn failed_reclamation_releases_its_gate_for_later_retries() {
    let inner = InMemory::new();
    let owner = owner_key("acme", &OwnerScope::organization("acme")).unwrap();
    let orphan = ObjectPath::from(format!(".pnpr-artifacts/v0/{owner}/blobs/orphan"));
    inner.put(&orphan, PutPayload::from_static(b"orphan")).await.unwrap();
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner,
        commit_before_error: false,
        fail_deletes: true,
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
    let publication = artifact_operation_id().unwrap();
    store.begin_publication(&publication).await.unwrap();
    store.reserve_quota(&owner, 6).await.unwrap();
    store.finish_publication(&publication, true).await.unwrap();

    store.try_reclaim_unreferenced_blobs().await.unwrap_err();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert!(usage.reclamation.is_none());
    assert!(usage.reclamation_needed);
    assert!(backend.head(&orphan).await.is_ok());
}

/// A renewal waits for the lock a local usage mutation holds, and that
/// mutation belongs to the publication being renewed. Waiting for it between
/// polls of the publication would leave each waiting on the other for good.
#[tokio::test]
async fn a_renewal_waiting_for_the_lock_does_not_stop_the_publication() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    store.begin_publication("a-publication").await.unwrap();
    let lock_path =
        storage.path().join(super::super::ARTIFACT_CACHE_DIR).join(".locks").join("usage.lock");
    let holding_the_lock = async {
        let _lock = super::super::acquire_artifact_lock(lock_path).await.unwrap();
        // Long enough that renewals tick while the lock is held, which is what
        // a publication does across every usage mutation it makes.
        tokio::time::sleep(super::super::ARTIFACT_LOCK_POLL_INTERVAL * 4).await;
        "the work ran to the end"
    };

    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        store.while_renewing("a-publication", Duration::from_millis(1), holding_the_lock),
    )
    .await
    .expect("the publication is polled while a renewal waits for the lock it holds");

    assert_eq!(outcome, "the work ran to the end");
}

/// A publication reaching several machines can retake some scopes and then find
/// one gone. What it retook names an artifact it is about to remove, so it puts
/// those back too rather than refusing later artifacts on behalf of one that is
/// not there.
#[tokio::test]
async fn recovery_that_loses_gives_back_what_it_had_retaken() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-arm64-node22-glibc2.17", "pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // The second of the two scopes went to somebody else; the first is free, so
    // recovery retakes it before finding the second gone.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/linux-x64-node22"),
            b"an artifact published meanwhile".to_vec(),
        )
        .await
        .unwrap();

    store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-arm64-node22"), 128)
            .await
            .unwrap()
            .is_none(),
        "the scope it retook does not stay held for an artifact it removed",
    );
}

/// Being written off lets reclamation run beside a publication, and before its
/// envelope is stored the blobs it uploaded are referenced by nothing. An
/// envelope naming files that are gone is worse than no artifact, so it is
/// taken out rather than served.
#[tokio::test]
async fn recovery_refuses_an_artifact_whose_blobs_were_collected() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/ours");
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&ours.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    // Stored, with the blob it names collected while the publication ran.
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(
        matches!(error, RegistryError::Internal { .. }),
        "the publication is told its artifact cannot stand, got {error:?}",
    );
    assert!(
        store.read_object_bounded(&variant, 4096).await.unwrap().is_none(),
        "and nothing is left naming files that are not there",
    );
}

/// Renewing says nothing about a publication that has already finished, since
/// its registration is gone and putting a time back would leave one nothing
/// removes.
#[tokio::test]
async fn renewing_a_publication_that_finished_records_nothing() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();

    store.renew_publication("a-publication-that-finished").await.unwrap();

    let usage: ArtifactUsage = serde_json::from_slice(
        &store.read_object_bounded(".locks/usage.json", 1 << 20).await.unwrap().unwrap_or_default(),
    )
    .unwrap_or_default();
    assert!(usage.active_publication_times.is_empty());
}
