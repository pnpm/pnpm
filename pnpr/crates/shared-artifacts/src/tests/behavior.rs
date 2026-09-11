use super::{
    Arc, ArtifactBlobRequest, ArtifactCandidate, ArtifactSubject, ArtifactUsage, AtomicUsize,
    FailArtifactWrites, FailOnly, HostedStoreConfig, InMemory, MAX_VARIANTS_PER_CANDIDATE,
    ObjectPath, ObjectStore, ObjectStoreExt, Ordering, OwnerScope, RegistryError,
    ResolveArtifactsRequest, SharedArtifactStore, TempDir, is_variant_file, lookup, publication,
    publication_for_platform, publication_tagged, publication_with_blob,
    workspace_task_publication,
};
use futures_util::StreamExt as _;

#[test]
fn variant_files_have_canonical_envelope_digest_names() {
    let digest = "a".repeat(64);
    assert!(is_variant_file(&format!("{digest}.json")));
    assert!(!is_variant_file(&format!("{digest}.json.tmp")));
    assert!(!is_variant_file(&format!("{}.json", "A".repeat(64))));
}

#[tokio::test]
async fn local_store_uses_the_cache_layout_and_round_trips_artifacts() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let request = publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/linux");
    let integrity = request.blobs[0].integrity.clone();

    assert!(store.publish("acme", request.clone()).await.unwrap());
    assert!(!store.publish("acme", request).await.unwrap());

    let response =
        store.resolve("acme", &serde_json::to_vec(&lookup("acme")).unwrap()).await.unwrap();
    assert_eq!(response.artifacts.len(), 1);
    assert_eq!(response.artifacts[0].variants.len(), 1);

    let mut blob = store
        .read_blob(
            "acme",
            &serde_json::to_vec(&ArtifactBlobRequest {
                owner: OwnerScope::organization("acme"),
                integrity,
            })
            .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = blob.stream.next().await {
        bytes.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(bytes, b"shared addon");
    assert!(storage.path().join("shared-artifacts/v0/.locks/usage.json").is_file());
}

#[tokio::test]
async fn workspace_task_subjects_round_trip_through_the_store() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let request = workspace_task_publication();

    assert!(store.publish("acme", request).await.unwrap());
    let response = store
        .resolve(
            "acme",
            &serde_json::to_vec(&ResolveArtifactsRequest {
                candidates: vec![ArtifactCandidate {
                    key: "workspace-task:v1:inputs=abc".to_string(),
                    subject: ArtifactSubject::workspace_task("packages/app", "build"),
                    owner: OwnerScope::organization("acme"),
                }],
            })
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.artifacts.len(), 1);
    assert_eq!(response.artifacts[0].variants.len(), 1);
}

#[tokio::test]
async fn committed_envelope_writes_that_report_failure_remain_charged() {
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
    let request = publication("ci/ambiguous-envelope");
    // The envelope reached the store while reporting failure, and the scope it
    // reaches stays claimed for it, so both are charged.
    let scope_marker = request.envelope.digest().unwrap().len() as u64;
    let expected_usage = serde_json::to_vec(&request.envelope).unwrap().len() as u64 + scope_marker;

    store.publish("acme", request).await.unwrap_err();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.global_bytes, expected_usage);
    assert_eq!(usage.owner_bytes.values().copied().sum::<u64>(), expected_usage);
}

/// A second build for a claimed slot is refused rather than stored beside the
/// first. See [`RegistryError::ArtifactAlreadyPublished`] for why.
#[tokio::test]
async fn a_second_artifact_cannot_claim_a_taken_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication("ci/first")).await.unwrap());

    let error = store.publish("acme", publication("ci/second")).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
    let response =
        store.resolve("acme", &serde_json::to_vec(&lookup("acme")).unwrap()).await.unwrap();
    assert_eq!(response.artifacts[0].variants.len(), 1, "the first artifact still stands");
}

/// An artifact stored under its envelope digest claims its slot whatever order
/// its tags are written in, and however late it sorts in the listing. Either
/// would otherwise leave an occupied slot looking free.
#[tokio::test]
async fn a_legacy_artifact_claims_its_slot_whatever_its_order_or_position() {
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17", "pnpm:v1:linux-arm64-node22-glibc2.17"];
    let reversed = [tags[1], tags[0]];

    for (label, buried) in [("reordered", false), ("buried", true)] {
        let storage = TempDir::new().unwrap();
        let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
        let legacy = publication_tagged("ci/legacy", &tags);
        let (payload, _) = legacy.envelope.decode_payload().unwrap();
        let owner = super::super::owner_key("acme", &payload.owner).unwrap();
        let entry = super::super::entry_digest(&legacy.key, &payload.subject);
        // Named to sort before the matching one, and more of them than a
        // lookup would scan.
        let filler_count = if buried { MAX_VARIANTS_PER_CANDIDATE + 2 } else { 0 };
        for index in 0..filler_count {
            let filler = publication_tagged(
                &format!("ci/filler/{index}"),
                &[&format!("pnpm:v1:linux-x64-node22-glibc2.{index}")],
            );
            store
                .create_object(
                    &format!("{owner}/entries/{entry}/{index:064x}.json"),
                    serde_json::to_vec(&filler.envelope).unwrap(),
                )
                .await
                .unwrap();
        }
        store
            .create_object(
                &format!("{owner}/entries/{entry}/{}.json", "f".repeat(64)),
                serde_json::to_vec(&legacy.envelope).unwrap(),
            )
            .await
            .unwrap();

        let error =
            store.publish("acme", publication_tagged("ci/second", &reversed)).await.unwrap_err();

        assert!(
            matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
            "{label}: expected a conflict, got {error:?}",
        );
    }
}

/// An artifact stored under its envelope digest claims its slot too, or a store
/// already holding one would leave it replaceable.
#[tokio::test]
async fn an_artifact_stored_under_the_older_name_still_claims_its_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let first = publication("ci/first");
    let envelope_bytes = serde_json::to_vec(&first.envelope).unwrap();
    let (payload, _) = first.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&first.key, &payload.subject);
    let envelope_digest = first.envelope.digest().unwrap();
    store
        .create_object(&format!("{owner}/entries/{entry}/{envelope_digest}.json"), envelope_bytes)
        .await
        .unwrap();

    let error = store.publish("acme", publication("ci/second")).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// Two publications can both find the slot empty, so losing the create is not
/// by itself an idempotent retry: whoever won may have stored something else.
#[tokio::test]
async fn losing_a_race_for_a_slot_is_not_reported_as_idempotent() {
    let winner = publication("ci/winner");
    let (payload, _) = winner.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&winner.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);

    // The loser passes the pre-check, then finds the slot taken at create time.
    let racing: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        commit_before_error: true,
        fail_deletes: false,
        fail_next_quota_write: None,
        claim_slot_first: Some((
            format!(".pnpr-artifacts/v0/{owner}/entries/{entry}/{slot}.json"),
            serde_json::to_vec(&winner.envelope).unwrap(),
        )),
        fail_slot_read_after_first: None,
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: None,
        usage_writes: None,
    });
    let racing = SharedArtifactStore::new(
        &HostedStoreConfig::ObjectStore { store: racing, prefix: String::new() },
        TempDir::new().unwrap().path(),
    )
    .unwrap();

    let error = racing.publish("acme", publication("ci/loser")).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// Markers are written one at a time and the scan stops at the first store
/// error, so a marker only says some artifact was reached. Taking that for
/// proof would let the next publication claim a scope a variant nobody reached
/// still holds.
#[tokio::test]
async fn a_backfill_that_did_not_finish_runs_again() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let stored = publication_tagged("ci/stored", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
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
    // What a backfill that stopped before its sentinel leaves behind.
    store
        .create_object(&format!("{owner}/entries/{entry}/scopes/darwin-x64-node22"), b"x".to_vec())
        .await
        .unwrap();

    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/raised", &raised)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the unreached variant still holds the machines it reaches, got {error:?}",
    );
}

/// A registration with no time is stamped, and the stamp has to reach the
/// store even when the pass that made it goes on to refuse something: a stamp
/// held only in memory is re-made on the next read, and the registration then
/// outlives every expiry that would have written it off.
#[tokio::test]
async fn a_stamp_survives_the_pass_that_refused_on_its_account() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let names: Vec<String> = (0..super::super::MAX_ACTIVE_PUBLICATIONS)
        .map(|index| format!("untimed-{index}"))
        .collect();
    store
        .create_object(
            ".locks/usage.json",
            serde_json::to_vec(&serde_json::json!({
                "global_bytes": 0,
                "owner_bytes": {},
                "active_publications": names,
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    // Refused: the limit is full of registrations that have only just been
    // stamped, so none of them is old enough to write off yet.
    let error = store
        .publish("acme", publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("concurrency limit reached"), "{error}");

    let usage: ArtifactUsage = serde_json::from_slice(
        &store.read_object_bounded(".locks/usage.json", 1 << 20).await.unwrap().unwrap(),
    )
    .unwrap();
    assert_eq!(
        usage.active_publication_times.len(),
        super::super::MAX_ACTIVE_PUBLICATIONS,
        "the stamps are on disk, so the hour they are measured against has started",
    );
}

/// Legacy variants can reach a scope another already reached, and each repeat
/// would otherwise cost a reservation and a release against the usage document.
#[tokio::test]
async fn a_backfill_writes_each_marker_once_however_many_variants_reach_it() {
    let usage_writes = Arc::new(AtomicUsize::new(0));
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
        fail_only: Some(FailOnly::WriteOf(String::new())),
        usage_writes: Some(Arc::clone(&usage_writes)),
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    // Two artifacts stored before markers existed, whose tags differ only in a
    // floor — so they reach the same scope, which is the overlap markers stop.
    let floors = ["pnpm:v1:linux-x64-node22-glibc2.17", "pnpm:v1:linux-x64-node22-glibc2.31"];
    let stored = publication_tagged("ci/stored", &floors[..1]);
    let (payload, _) = stored.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&stored.key, &payload.subject);
    for floor in floors {
        let legacy = publication_tagged("ci/stored", &[floor]);
        let (payload, _) = legacy.envelope.decode_payload().unwrap();
        let slot = super::super::compatibility_slot(&payload.compatibility);
        store
            .create_object(
                &format!("{owner}/entries/{entry}/{slot}.json"),
                serde_json::to_vec(&legacy.envelope).unwrap(),
            )
            .await
            .unwrap();
    }
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-arm64-node22-glibc2.17"]);
    let prepared = super::super::prepare_publication("acme", &ours).unwrap();
    usage_writes.store(0, Ordering::SeqCst);

    store.backfill_scopes(&prepared).await.unwrap();

    assert_eq!(
        usage_writes.load(Ordering::SeqCst),
        1,
        "the one marker both variants reach is reserved once, not once each",
    );
}

/// A store error says nothing about whether the marker landed, so a marker
/// that did stays charged: letting storage outgrow a quota is the worse way to
/// be wrong, and reclamation gives back what turns out not to be there.
#[tokio::test]
async fn a_marker_written_by_a_failing_write_stays_charged() {
    let stored = publication_tagged("ci/stored", &["pnpm:v1:linux-arm64-node22-glibc2.17"]);
    let (payload, _) = stored.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&stored.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let marker = format!("{owner}/entries/{entry}/scopes/linux-arm64-node22");
    let backend: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
        inner: InMemory::new(),
        // The write lands and then reports a failure, which is the case a
        // conditional create cannot tell from one that stored nothing.
        commit_before_error: true,
        fail_deletes: false,
        fail_next_quota_write: None,
        claim_slot_first: None,
        fail_slot_read_after_first: None,
        publish_overlapping_after_create: None,
        fail_reads_of: None,
        fail_scope_writes: false,
        fail_only: Some(FailOnly::WriteOf(format!(".pnpr-artifacts/v0/{marker}"))),
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    // An entry holding no markers, which is what a backfill is for.
    let envelope = serde_json::to_vec(&stored.envelope).unwrap();
    let stored_bytes = envelope.len() as u64;
    store.create_object(&format!("{owner}/entries/{entry}/{slot}.json"), envelope).await.unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let prepared = super::super::prepare_publication("acme", &ours).unwrap();

    let error = store.backfill_scopes(&prepared).await.unwrap_err();

    assert!(matches!(error, RegistryError::ObjectStore(_)), "{error:?}");
    let usage: ArtifactUsage = serde_json::from_slice(
        &backend
            .get(&ObjectPath::from(".pnpr-artifacts/v0/quota.json"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
    )
    .unwrap();
    let marker_bytes = stored.envelope.digest().unwrap().len() as u64;
    assert_eq!(
        usage.owner_bytes.values().copied().sum::<u64>(),
        stored_bytes + marker_bytes,
        "the marker that is there is charged for",
    );
}

/// A marker the backfill cannot write leaves nothing charged for it: only a
/// pass over what is stored can say whether the bytes are there, and an owner
/// charged for what may not be is refused publications that fit.
#[tokio::test]
async fn a_backfill_that_cannot_write_a_marker_gives_its_charge_back() {
    let stored = publication_tagged("ci/stored", &["pnpm:v1:linux-arm64-node22-glibc2.17"]);
    let (payload, _) = stored.envelope.decode_payload().unwrap();
    let owner = super::super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::super::entry_digest(&stored.key, &payload.subject);
    let slot = super::super::compatibility_slot(&payload.compatibility);
    let marker = format!("{owner}/entries/{entry}/scopes/linux-arm64-node22");
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
        fail_only: Some(FailOnly::WriteOf(format!(".pnpr-artifacts/v0/{marker}"))),
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    // An entry holding no markers, which is what a backfill is for.
    let envelope = serde_json::to_vec(&stored.envelope).unwrap();
    let stored_bytes = envelope.len() as u64;
    store.create_object(&format!("{owner}/entries/{entry}/{slot}.json"), envelope).await.unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let prepared = super::super::prepare_publication("acme", &ours).unwrap();

    let error = store.backfill_scopes(&prepared).await.unwrap_err();

    assert!(matches!(error, RegistryError::ObjectStore(_)), "{error:?}");
    let usage: ArtifactUsage = serde_json::from_slice(
        &backend
            .get(&ObjectPath::from(".pnpr-artifacts/v0/quota.json"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        usage.owner_bytes.values().copied().sum::<u64>(),
        stored_bytes,
        "the artifact that is stored is charged, and the marker that is not is not",
    );
}

/// The markers an entry is given for artifacts already stored are objects like
/// any other, so an owner with no room left cannot write them either.
#[tokio::test]
async fn an_owner_with_no_room_cannot_have_markers_written_for_them() {
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

    let full =
        SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap().with_limits(1, 1);
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let prepared = super::super::prepare_publication("acme", &ours).unwrap();

    let error = full.backfill_scopes(&prepared).await.unwrap_err();

    assert!(error.to_string().contains("quota exceeded"), "{error}");
    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-arm64-node22"), 128)
            .await
            .unwrap()
            .is_none(),
        "nothing is written for an owner who has no room for it",
    );
}

#[tokio::test]
async fn the_variant_limit_is_applied_at_read_time() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    for index in 0..MAX_VARIANTS_PER_CANDIDATE + 2 {
        assert!(store.publish("acme", publication_for_platform(index)).await.unwrap());
    }

    let response =
        store.resolve("acme", &serde_json::to_vec(&lookup("acme")).unwrap()).await.unwrap();

    assert_eq!(response.artifacts[0].variants.len(), MAX_VARIANTS_PER_CANDIDATE);
}

#[tokio::test]
async fn another_owner_cannot_probe_artifacts() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication("ci/acme")).await.unwrap());

    let response =
        store.resolve("mallory", &serde_json::to_vec(&lookup("acme")).unwrap()).await.unwrap();

    assert!(response.artifacts.is_empty());
}
