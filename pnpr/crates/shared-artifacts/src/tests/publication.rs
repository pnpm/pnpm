use super::{
    Arc, ArtifactBlobRequest, ArtifactUsage, ArtifactVariant, FailArtifactWrites, FailOnly,
    HostedStoreConfig, InMemory, Instant, MAX_RESOLVE_RESPONSE_SIZE, ObjectPath, ObjectStore,
    ObjectStoreExt, OwnerScope, PutPayload, RegistryError, ResolveArtifactsResponse, ResolveBudget,
    ResolvedArtifact, SharedArtifactStore, SignedArtifactEnvelope, TempDir, artifact_operation_id,
    owner_key, publication, publication_tagged, publication_with_blob,
};
use futures_util::StreamExt as _;

#[test]
fn resolve_budget_bounds_combined_scanned_and_serialized_bytes() {
    let empty_response_size =
        serde_json::to_vec(&ResolveArtifactsResponse { artifacts: Vec::new() }).unwrap().len();
    let mut scan_budget = ResolveBudget { used_bytes: 0 };
    scan_budget.add_scan(MAX_RESOLVE_RESPONSE_SIZE as u64).unwrap();
    assert!(scan_budget.add_scan(1).is_err());

    let artifact = ResolvedArtifact {
        key: "dependency-side-effects:v1:deps=abc".to_string(),
        variants: vec![ArtifactVariant {
            envelope: SignedArtifactEnvelope {
                algorithm: "ecdsa-p256-sha256".to_string(),
                key_id: "key".to_string(),
                payload: "e30=".to_string(),
                signature: "eA==".to_string(),
            },
        }],
    };
    let mut response_budget = ResolveBudget { used_bytes: MAX_RESOLVE_RESPONSE_SIZE };
    assert!(response_budget.add_response(&artifact, false).is_err());

    let mut combined_budget = ResolveBudget { used_bytes: empty_response_size };
    combined_budget.add_scan((MAX_RESOLVE_RESPONSE_SIZE - empty_response_size) as u64).unwrap();
    assert!(combined_budget.add_response(&artifact, false).is_err());
}

#[tokio::test]
async fn concurrent_duplicate_publications_are_charged_once() {
    let backend: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let first = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let second = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let request = publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/duplicate");
    // One marker between them: whichever claims the scope first, the other
    // recognises it rather than writing a second.
    let expected = b"shared addon".len() as u64
        + serde_json::to_vec(&request.envelope).unwrap().len() as u64
        + request.envelope.digest().unwrap().len() as u64;

    let first_publish = first.publish("acme", request.clone());
    let second_publish = second.publish("acme", request);
    let (first, second) = tokio::join!(first_publish, second_publish);

    assert_ne!(first.unwrap(), second.unwrap());
    let usage_path = object_store::path::Path::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.global_bytes, expected);
}

#[tokio::test]
async fn committed_blob_writes_without_an_envelope_are_reclaimed() {
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: true,
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
    let request =
        publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/ambiguous-commit");
    store.publish("acme", request).await.unwrap_err();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert_eq!(usage.owner_bytes.values().copied().sum::<u64>(), 0);
    let mut objects = backend.list(None);
    let mut physical_bytes = 0_u64;
    while let Some(object) = objects.next().await {
        let object = object.unwrap();
        if !object.location.as_ref().ends_with("/quota.json") {
            physical_bytes += object.size;
        }
    }
    assert_eq!(physical_bytes, 0);
}

#[tokio::test]
async fn reclamation_waits_for_publications_on_other_replicas() {
    let backend: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let first = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let second = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let first_publication = artifact_operation_id().unwrap();
    let second_publication = artifact_operation_id().unwrap();
    first.begin_publication(&first_publication).await.unwrap();
    second.begin_publication(&second_publication).await.unwrap();

    let owner = owner_key("acme", &OwnerScope::organization("acme")).unwrap();
    let orphan = ObjectPath::from(format!(".pnpr-artifacts/v0/{owner}/blobs/orphan"));
    backend.put(&orphan, PutPayload::from_static(b"orphan")).await.unwrap();
    first.reserve_quota(&owner, 6).await.unwrap();

    first.finish_publication(&first_publication, true).await.unwrap();
    first.try_reclaim_unreferenced_blobs().await.unwrap();
    assert!(backend.head(&orphan).await.is_ok());

    second.finish_publication(&second_publication, false).await.unwrap();
    second.try_reclaim_unreferenced_blobs().await.unwrap();
    assert!(matches!(backend.head(&orphan).await, Err(object_store::Error::NotFound { .. })));
    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert!(usage.active_publications.is_empty());
    assert!(!usage.reclamation_needed);
    assert!(usage.reclamation.is_none());
}

#[tokio::test]
async fn reclamation_preserves_blobs_referenced_by_committed_envelopes() {
    let backend: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let request = publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/referenced");
    let integrity = request.blobs[0].integrity.clone();
    store.publish("acme", request).await.unwrap();

    let publication = artifact_operation_id().unwrap();
    store.begin_publication(&publication).await.unwrap();
    let owner = owner_key("acme", &OwnerScope::organization("acme")).unwrap();
    let orphan = ObjectPath::from(format!(".pnpr-artifacts/v0/{owner}/blobs/orphan"));
    backend.put(&orphan, PutPayload::from_static(b"orphan")).await.unwrap();
    store.reserve_quota(&owner, 6).await.unwrap();
    store.finish_publication(&publication, true).await.unwrap();
    store.try_reclaim_unreferenced_blobs().await.unwrap();

    assert!(matches!(backend.head(&orphan).await, Err(object_store::Error::NotFound { .. })));
    let blob = store
        .read_blob(
            "acme",
            &serde_json::to_vec(&ArtifactBlobRequest {
                owner: OwnerScope::organization("acme"),
                integrity,
            })
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(blob.is_some());
}

/// A store may hold several artifacts for one slot. Republishing any of them is
/// a retry, so the whole slot is searched for the incoming envelope before
/// another one is reported: the second is no less already-published than the
/// first.
/// An entry can hold artifacts that apply to one consumer: a store written
/// before this rule, or one whose withdrawal could not finish. Neither of them
/// is *the* artifact for those consumers, so republishing either is refused
/// rather than reported as already published — which would hide the state and
/// leave nobody to repair it.
#[tokio::test]
async fn an_entry_crowded_with_overlapping_artifacts_refuses_publication() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let universal = publication("ci/universal");
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let tagged = publication_tagged("ci/tagged", &tags);
    let (payload, _) = universal.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&universal.key, &payload.subject);
    for (name, request) in [("a".repeat(64), &universal), ("b".repeat(64), &tagged)] {
        store
            .create_object(
                &format!("{owner}/entries/{entry}/{name}.json"),
                serde_json::to_vec(&request.envelope).unwrap(),
            )
            .await
            .unwrap();
    }

    // Each holds the scope it reaches, so only looking at its own would report
    // both as already published.
    for republished in [publication("ci/universal"), publication_tagged("ci/tagged", &tags)] {
        let error = store.publish("acme", republished).await.unwrap_err();
        assert!(
            matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
            "republishing into a crowded entry is refused, got {error:?}",
        );
    }
    let error = store.publish("acme", publication("ci/third")).await.unwrap_err();
    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "a genuinely new artifact still conflicts, got {error:?}",
    );
}

/// The rule refuses artifacts that could serve one machine, not artifacts for
/// one input key: a publisher still fills out its matrix.
#[tokio::test]
async fn artifacts_no_consumer_can_share_are_published_side_by_side() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    for tag in [
        "pnpm:v1:linux-x64-node22-glibc2.17",
        "pnpm:v1:linux-arm64-node22-glibc2.17",
        "pnpm:v1:linux-x64-node24-glibc2.17",
        "pnpm:v1:darwin-arm64-node22-macos13.0",
        "pnpm:v1:win32-x64-node22-windows10.0.17763",
    ] {
        assert!(
            store.publish("acme", publication_tagged("ci/matrix", &[tag])).await.unwrap(),
            "{tag} should not conflict with any other platform",
        );
    }
}

/// Reclamation is what gives back the scopes a failed publication claimed, and
/// it runs only when no publication is in flight. A publication that could not
/// unregister itself would hold that shut forever, so a registration older than
/// any publication can plausibly take is dropped.
#[tokio::test]
async fn a_publication_that_never_finished_stops_holding_reclamation_shut() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let stranded = super::super::artifact_operation_id().unwrap();
    let long_ago =
        super::super::registered_now() - super::super::ACTIVE_PUBLICATION_EXPIRY.as_secs() - 1;
    let entry = {
        let ours = publication_tagged("ci/ours", &tags);
        let (payload, _) = ours.envelope.decode_payload().unwrap();
        super::super::entry_digest(&ours.key, &payload.subject)
    };
    let owner = super::super::owner_key("acme", &OwnerScope::organization("acme")).unwrap();
    let marker = format!("{owner}/entries/{entry}/scopes/linux-x64-node22");
    store.create_object(&marker, b"an artifact nobody stored".to_vec()).await.unwrap();
    store
        .create_object(
            ".locks/usage.json",
            serde_json::to_vec(&serde_json::json!({
                "global_bytes": 0,
                "owner_bytes": {},
                "active_publications": [stranded],
                "active_publication_times": { stranded: long_ago },
                "reclamation_needed": true,
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    store.try_reclaim_unreferenced_blobs().await.unwrap();

    assert!(
        store.read_object_bounded(&marker, 128).await.unwrap().is_none(),
        "the scope goes back once the publication holding the gate is written off",
    );
}

/// Registrations nobody will remove would otherwise fill the concurrency limit
/// and refuse publications that could run.
#[tokio::test]
async fn publications_that_never_finished_stop_filling_the_limit() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let long_ago =
        super::super::registered_now() - super::super::ACTIVE_PUBLICATION_EXPIRY.as_secs() - 1;
    let names: Vec<String> = (0..super::super::MAX_ACTIVE_PUBLICATIONS)
        .map(|index| format!("stranded-{index}"))
        .collect();
    let times: serde_json::Map<String, serde_json::Value> =
        names.iter().map(|name| (name.clone(), serde_json::json!(long_ago))).collect();
    store
        .create_object(
            ".locks/usage.json",
            serde_json::to_vec(&serde_json::json!({
                "global_bytes": 0,
                "owner_bytes": {},
                "active_publications": names,
                "active_publication_times": times,
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        store
            .publish("acme", publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]))
            .await
            .unwrap(),
        "a full set of registrations nobody will remove does not refuse a publication",
    );
}

/// A replica that does not keep registration times shares this document, so a
/// publication can be registered without one. Writing it off at once would let
/// a collector run beside one still in flight, so it is stamped instead — and
/// the pass reports that it changed something, because a stamp nobody persists
/// is re-made on every read and outlives every expiry.
#[tokio::test]
async fn a_publication_registered_without_a_time_is_stamped_rather_than_written_off() {
    let mut usage: ArtifactUsage = serde_json::from_value(serde_json::json!({
        "global_bytes": 0,
        "owner_bytes": {},
        "active_publications": ["a-publication-in-flight"],
    }))
    .unwrap();

    assert!(
        super::super::expire_stranded_publications(&mut usage),
        "the stamp has to be written down",
    );

    assert!(usage.active_publications.contains("a-publication-in-flight"));
    assert!(usage.active_publication_times.contains_key("a-publication-in-flight"));
}

/// Registering again is what makes the recovery's reads mean anything. A
/// publication that cannot register cannot look, so it does not leave an
/// envelope standing for blobs a collector may already have taken.
#[tokio::test]
async fn a_publication_that_cannot_register_again_does_not_leave_its_artifact() {
    let request = publication("ci/unregistrable");
    let prepared = super::super::prepare_publication("acme", &request).unwrap();
    let variant = format!(".pnpr-artifacts/v0/{}", prepared.variant_path);
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
        fail_only: Some(FailOnly::RegistrationAfter(variant.clone())),
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let prepared = super::super::PreparedPublication {
        started: Instant::now().checked_sub(super::super::ACTIVE_PUBLICATION_EXPIRY).unwrap(),
        ..prepared
    };
    let mut reclamation_needed = false;
    let mut created = Vec::new();

    let error = store
        .publish_reserving(prepared, "a-publication", &mut reclamation_needed, &mut created)
        .await
        .unwrap_err();

    assert!(matches!(error, RegistryError::ObjectStore(_)), "{error:?}");
    assert!(
        backend.head(&ObjectPath::from(variant.as_str())).await.is_err(),
        "the artifact it could not vouch for is taken back out",
    );
}

/// A publication says at intervals that it is still working, so one that is
/// merely slow is never mistaken for one that stopped — which is what keeps a
/// collector from running beside it and taking what it has not finished with.
#[tokio::test]
async fn a_publication_still_working_says_so() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let publication = super::super::artifact_operation_id().unwrap();
    let long_ago =
        super::super::registered_now() - super::super::ACTIVE_PUBLICATION_EXPIRY.as_secs() - 1;
    store
        .create_object(
            ".locks/usage.json",
            serde_json::to_vec(&serde_json::json!({
                "global_bytes": 0,
                "owner_bytes": {},
                "active_publications": [publication.clone()],
                "active_publication_times": { publication.clone(): long_ago },
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    store.renew_publication(&publication).await.unwrap();

    let mut usage: ArtifactUsage = serde_json::from_slice(
        &store.read_object_bounded(".locks/usage.json", 1 << 20).await.unwrap().unwrap(),
    )
    .unwrap();
    assert!(
        !super::super::expire_stranded_publications(&mut usage),
        "a registration that has just spoken is not written off",
    );
    assert!(usage.active_publications.contains(&publication));
}

/// A retried publication of the identical envelope is not an attempt to replace
/// anything, so it stays idempotent rather than becoming a conflict.
#[tokio::test]
async fn republishing_the_same_artifact_stays_idempotent() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication("ci/first")).await.unwrap());

    assert!(!store.publish("acme", publication("ci/first")).await.unwrap());
}
