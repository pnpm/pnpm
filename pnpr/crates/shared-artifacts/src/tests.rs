use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{StreamExt as _, stream::BoxStream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    ObjectStoreExt, PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions,
    memory::InMemory, path::Path as ObjectPath,
};
use pnpm_shared_artifact_protocol::{
    ARTIFACT_KIND, ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, ArtifactSubject, ArtifactVariant, BuilderProfile,
    CompatibilityConstraints, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE, OwnerScope,
    PackageIdentity, PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse,
    ResolvedArtifact, SIGNATURE_ALGORITHM, SignedArtifactEnvelope, WORKSPACE_TASK_ARTIFACT_KIND,
};
use pnpr_config::{HostedStoreConfig, normalize_key_prefix};
use pnpr_error::RegistryError;
use sha2::{Digest as _, Sha512};
use tempfile::TempDir;

use super::{
    ArtifactUsage, ResolveBudget, SharedArtifactStore, artifact_operation_id, is_variant_file,
    is_write_conflict, owner_key,
};

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

#[test]
fn variant_files_have_canonical_envelope_digest_names() {
    let digest = "a".repeat(64);
    assert!(is_variant_file(&format!("{digest}.json")));
    assert!(!is_variant_file(&format!("{digest}.json.tmp")));
    assert!(!is_variant_file(&format!("{}.json", "A".repeat(64))));
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
        let owner = super::owner_key("acme", &payload.owner).unwrap();
        let entry = super::entry_digest(&legacy.key, &payload.subject);
        if buried {
            // Named to sort before the matching one, and more of them than a
            // lookup would scan.
            for index in 0..MAX_VARIANTS_PER_CANDIDATE + 2 {
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

/// Losing the race and then failing to read the winner must not leave the loser
/// charged for an envelope it did not store: that debt never comes back, and
/// enough of it starts refusing publications that fit.
#[tokio::test]
async fn a_failed_reread_after_a_lost_race_still_releases_the_quota() {
    let winner = publication("ci/winner");
    let (payload, _) = winner.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&winner.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&universal.key, &payload.subject);
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

/// Matching a tag set is order-independent, so two orderings are the same
/// constraint and must not be two slots — otherwise a publisher reopens the
/// swap simply by listing the same tags the other way round.
#[tokio::test]
async fn tag_order_does_not_open_a_second_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17", "pnpm:v1:linux-arm64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap());

    let reversed = [tags[1], tags[0]];
    let error =
        store.publish("acme", publication_tagged("ci/second", &reversed)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// A tagged artifact outranks a universal one for every consumer its tag fits,
/// so publishing one over a universal artifact would decide what those
/// consumers run without ever refilling a slot.
#[tokio::test]
async fn a_tagged_artifact_cannot_supersede_a_universal_one() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication("ci/first")).await.unwrap());

    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let error = store.publish("acme", publication_tagged("ci/second", &tags)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

#[tokio::test]
async fn a_universal_artifact_cannot_supersede_a_tagged_one() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap());

    let error = store.publish("acme", publication("ci/second")).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
}

/// A consumer meeting the higher floor meets the lower one as well, so both
/// artifacts would apply to it and ranking would pick between them.
#[tokio::test]
async fn a_second_floor_for_one_platform_cannot_open_a_second_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let floor = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/first", &floor)).await.unwrap());

    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/second", &raised)).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
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

/// An artifact stored under its envelope digest claims its slot too, or a store
/// already holding one would leave it replaceable.
#[tokio::test]
async fn an_artifact_stored_under_the_older_name_still_claims_its_slot() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let first = publication("ci/first");
    let envelope_bytes = serde_json::to_vec(&first.envelope).unwrap();
    let (payload, _) = first.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&first.key, &payload.subject);
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&winner.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);

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

/// The marker, not the variant path, is what two publications contend on, and
/// nothing of the refused one is written: it is turned away before it stores
/// anything, and what it lost to is left as it was.
#[tokio::test]
async fn a_scope_another_artifact_holds_refuses_publication() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let held = publication_tagged("ci/held", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let holder = held.envelope.digest().unwrap();
    let (payload, _) = held.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&held.key, &payload.subject);
    assert!(store.publish("acme", held).await.unwrap());

    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.31"]);
    let slot = {
        let (payload, _) = ours.envelope.decode_payload().unwrap();
        super::compatibility_slot(&payload.compatibility)
    };
    let error = store.publish("acme", ours).await.unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "expected a conflict, got {error:?}",
    );
    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/{slot}.json"), 4096)
            .await
            .unwrap()
            .is_none(),
        "nothing is written before the claim succeeds",
    );
    assert_eq!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-x64-node22"), 128)
            .await
            .unwrap()
            .as_deref(),
        Some(holder.as_bytes()),
        "and the marker it lost on still names the artifact holding it",
    );
}

/// A scope this publication reserved and did not keep has to go back, or the
/// artifact that should hold it could never be published.
#[tokio::test]
async fn a_publication_that_fails_gives_back_the_scopes_it_claimed() {
    let failing: Arc<dyn ObjectStore> = Arc::new(FailArtifactWrites {
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
    let store = SharedArtifactStore::new(
        &HostedStoreConfig::ObjectStore { store: failing, prefix: String::new() },
        TempDir::new().unwrap().path(),
    )
    .unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];

    store.publish("acme", publication_tagged("ci/first", &tags)).await.unwrap_err();

    let (payload, _) = publication_tagged("ci/first", &tags).envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&publication_tagged("ci/first", &tags).key, &payload.subject);
    assert!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-x64-node22"), 128)
            .await
            .unwrap()
            .is_none(),
        "the scope is free for the artifact that does get stored",
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&stored.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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

/// A marker that loses the create and is gone by the time it is read belongs to
/// nobody. Reading that as this artifact's own would store it reserving
/// nothing, and an overlapping artifact could follow it in.
#[tokio::test]
async fn a_scope_that_vanishes_after_the_create_is_not_taken_as_ours() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);

    assert_eq!(
        store.scope_marker(&owner, &entry, "linux-x64-node22", "any-digest").await.unwrap(),
        super::ScopeMarker::Gone,
        "an absent marker is nobody's, not this artifact's",
    );
}

/// A publication that claims scopes and then fails leaves them claimed: it
/// cannot tell its own abandoned marker from one a publication of the same
/// envelope is using right now. Reclamation runs when none is in flight, so it
/// can, and the scope goes back there.
#[tokio::test]
async fn a_scope_a_failed_publication_left_behind_is_reclaimed() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let marker = format!("{owner}/entries/{entry}/scopes/linux-x64-node22");
    // What a publication that claimed the scope and then failed leaves.
    store.create_object(&marker, b"an artifact nobody stored".to_vec()).await.unwrap();

    store.reclaim_unreferenced_blobs().await.unwrap();

    assert!(
        store.read_object_bounded(&marker, 128).await.unwrap().is_none(),
        "a scope no stored artifact holds goes back",
    );
    assert!(
        store.publish("acme", publication_tagged("ci/later", &tags)).await.unwrap(),
        "and the artifact that should hold it can be published",
    );
}

/// Reclamation drops what no artifact holds, not what a stored one does.
#[tokio::test]
async fn reclamation_keeps_the_scopes_a_stored_artifact_reaches() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    assert!(store.publish("acme", publication_tagged("ci/stored", &tags)).await.unwrap());

    store.reclaim_unreferenced_blobs().await.unwrap();

    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/raised", &raised)).await.unwrap_err();
    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the stored artifact still reaches its machines, got {error:?}",
    );
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

/// Once a crowded entry has been given its markers, each artifact holds the
/// scope it reaches, and looking only at its own would report both as already
/// published. Reaching the same machines from the other side of the vocabulary
/// is what a retry into such an entry has to be refused for.
#[tokio::test]
async fn a_retry_into_a_crowded_entry_is_refused_once_its_scopes_are_known() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let universal = publication("ci/universal");
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let tagged = publication_tagged("ci/tagged", &tags);
    let (payload, _) = universal.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&universal.key, &payload.subject);
    // Each under the name its own constraints give it, which is where a store
    // written when only identical constraints conflicted put them.
    for request in [&universal, &tagged] {
        let (payload, _) = request.envelope.decode_payload().unwrap();
        let slot = super::compatibility_slot(&payload.compatibility);
        store
            .create_object(
                &format!("{owner}/entries/{entry}/{slot}.json"),
                serde_json::to_vec(&request.envelope).unwrap(),
            )
            .await
            .unwrap();
    }
    // Gives the entry the markers its artifacts reach, and is itself refused.
    store.publish("acme", publication("ci/third")).await.unwrap_err();

    for republished in [publication("ci/universal"), publication_tagged("ci/tagged", &tags)] {
        let error = store.publish("acme", republished).await.unwrap_err();
        assert!(
            matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
            "a retry into a crowded entry is refused, got {error:?}",
        );
    }
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&stored.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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

/// Reclamation is what gives back the scopes a failed publication claimed, and
/// it runs only when no publication is in flight. A publication that could not
/// unregister itself would hold that shut forever, so a registration older than
/// any publication can plausibly take is dropped.
#[tokio::test]
async fn a_publication_that_never_finished_stops_holding_reclamation_shut() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let stranded = super::artifact_operation_id().unwrap();
    let long_ago = super::registered_now() - super::ACTIVE_PUBLICATION_EXPIRY.as_secs() - 1;
    let entry = {
        let ours = publication_tagged("ci/ours", &tags);
        let (payload, _) = ours.envelope.decode_payload().unwrap();
        super::entry_digest(&ours.key, &payload.subject)
    };
    let owner = super::owner_key("acme", &OwnerScope::organization("acme")).unwrap();
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
    let long_ago = super::registered_now() - super::ACTIVE_PUBLICATION_EXPIRY.as_secs() - 1;
    let names: Vec<String> =
        (0..super::MAX_ACTIVE_PUBLICATIONS).map(|index| format!("stranded-{index}")).collect();
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

    assert!(super::expire_stranded_publications(&mut usage), "the stamp has to be written down");

    assert!(usage.active_publications.contains("a-publication-in-flight"));
    assert!(usage.active_publication_times.contains_key("a-publication-in-flight"));
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
        storage.path().join(super::ARTIFACT_CACHE_DIR).join(".locks").join("usage.lock");
    let holding_the_lock = async {
        let _lock = super::acquire_artifact_lock(lock_path).await.unwrap();
        // Long enough that renewals tick while the lock is held, which is what
        // a publication does across every usage mutation it makes.
        tokio::time::sleep(super::ARTIFACT_LOCK_POLL_INTERVAL * 4).await;
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

/// A registration with no time is stamped, and the stamp has to reach the
/// store even when the pass that made it goes on to refuse something: a stamp
/// held only in memory is re-made on the next read, and the registration then
/// outlives every expiry that would have written it off.
#[tokio::test]
async fn a_stamp_survives_the_pass_that_refused_on_its_account() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let names: Vec<String> =
        (0..super::MAX_ACTIVE_PUBLICATIONS).map(|index| format!("untimed-{index}")).collect();
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
        super::MAX_ACTIVE_PUBLICATIONS,
        "the stamps are on disk, so the hour they are measured against has started",
    );
}

/// Registering again is what makes the recovery's reads mean anything. A
/// publication that cannot register cannot look, so it does not leave an
/// envelope standing for blobs a collector may already have taken.
#[tokio::test]
async fn a_publication_that_cannot_register_again_does_not_leave_its_artifact() {
    let request = publication("ci/unregistrable");
    let prepared = super::prepare_publication("acme", &request).unwrap();
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
    let prepared = super::PreparedPublication {
        started: Instant::now().checked_sub(super::ACTIVE_PUBLICATION_EXPIRY).unwrap(),
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

/// Writing off a publication that is merely slow lets reclamation give back
/// scopes it is still holding. Its artifact is stored all the same, so it takes
/// them back rather than being left reaching machines nothing says it reaches.
#[tokio::test]
async fn a_publication_written_off_while_running_takes_its_scopes_back() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let tags = ["pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
    let marker = format!("{owner}/entries/{entry}/scopes/linux-x64-node22");
    // The artifact is stored and its scope has been given back, which is where a
    // publication written off mid-flight finds things when it comes to finish.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/{slot}.json"),
            serde_json::to_vec(&ours.envelope).unwrap(),
        )
        .await
        .unwrap();

    store
        .recover_after_expiry(
            &owner,
            &entry,
            &format!("{owner}/entries/{entry}/{slot}.json"),
            &payload,
            &ours.envelope.digest().unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        store.read_object_bounded(&marker, 128).await.unwrap().as_deref(),
        Some(ours.envelope.digest().unwrap().as_bytes()),
        "the stored artifact reaches its machines again",
    );
    let raised = ["pnpm:v1:linux-x64-node22-glibc2.31"];
    let error = store.publish("acme", publication_tagged("ci/raised", &raised)).await.unwrap_err();
    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "and one reaching the same machines is refused again, got {error:?}",
    );
}

/// A scope can have gone to an artifact published while this one was written
/// off, and that artifact holds it. This one is then reaching a machine nothing
/// says it reaches, so it takes its own artifact back out rather than leaving
/// two that reach it.
#[tokio::test]
async fn a_publication_whose_scope_went_elsewhere_takes_its_artifact_back_out() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // Published while this one was written off, and holding the scope now.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/linux-x64-node22"),
            b"an artifact published meanwhile".to_vec(),
        )
        .await
        .unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the publication is told it lost, got {error:?}",
    );
    assert!(
        store.read_object_bounded(&variant, 4096).await.unwrap().is_none(),
        "and its artifact does not stay beside the one that holds the scope",
    );
}

/// The artifact goes out before the scopes it retook, so that a store error
/// between the two never leaves it resolvable while holding nothing.
#[tokio::test]
async fn a_recovery_that_cannot_remove_its_artifact_keeps_the_scopes_it_retook() {
    let tags = ["pnpm:v1:linux-arm64-node22-glibc2.17", "pnpm:v1:linux-x64-node22-glibc2.17"];
    let ours = publication_tagged("ci/ours", &tags);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
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
        fail_only: Some(FailOnly::DeleteOf(format!(".pnpr-artifacts/v0/{variant}"))),
        usage_writes: None,
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // Taken while this publication was written off, so the recovery loses — but
    // only after retaking the scope that sorts before it.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/linux-x64-node22"),
            b"an artifact published meanwhile".to_vec(),
        )
        .await
        .unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(matches!(error, RegistryError::ObjectStore(_)), "{error:?}");
    assert_eq!(
        store
            .read_object_bounded(&format!("{owner}/entries/{entry}/scopes/linux-arm64-node22"), 128)
            .await
            .unwrap()
            .as_deref(),
        Some(ours.envelope.digest().unwrap().as_bytes()),
        "the artifact that is still there still holds the scope it retook",
    );
}

/// The other form of the vocabulary reaches these machines too. A publication
/// that took the universal key while this tagged one was written off holds it
/// under a key this one never claims, so recovering only its own keys would
/// leave both stored.
#[tokio::test]
async fn recovery_sees_a_scope_taken_through_the_other_form() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-x64-node22-glibc2.17"]);
    let (payload, _) = ours.envelope.decode_payload().unwrap();
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
    let variant = format!("{owner}/entries/{entry}/{slot}.json");
    store.create_object(&variant, serde_json::to_vec(&ours.envelope).unwrap()).await.unwrap();
    // Reaches every machine, including the ones this artifact reaches, and it
    // took its key while this publication was written off.
    store
        .create_object(
            &format!("{owner}/entries/{entry}/scopes/universal"),
            b"an artifact reaching everything".to_vec(),
        )
        .await
        .unwrap();

    let error = store
        .recover_after_expiry(&owner, &entry, &variant, &payload, &ours.envelope.digest().unwrap())
        .await
        .unwrap_err();

    assert!(
        matches!(error, RegistryError::ArtifactAlreadyPublished { .. }),
        "the publication is told it lost, got {error:?}",
    );
    assert!(
        store.read_object_bounded(&variant, 4096).await.unwrap().is_none(),
        "and its artifact does not stay beside the one reaching the same machines",
    );
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&ours.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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

/// A publication says at intervals that it is still working, so one that is
/// merely slow is never mistaken for one that stopped — which is what keeps a
/// collector from running beside it and taking what it has not finished with.
#[tokio::test]
async fn a_publication_still_working_says_so() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    let publication = super::artifact_operation_id().unwrap();
    let long_ago = super::registered_now() - super::ACTIVE_PUBLICATION_EXPIRY.as_secs() - 1;
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
        !super::expire_stranded_publications(&mut usage),
        "a registration that has just spoken is not written off",
    );
    assert!(usage.active_publications.contains(&publication));
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&stored.key, &payload.subject);
    for floor in floors {
        let legacy = publication_tagged("ci/stored", &[floor]);
        let (payload, _) = legacy.envelope.decode_payload().unwrap();
        let slot = super::compatibility_slot(&payload.compatibility);
        store
            .create_object(
                &format!("{owner}/entries/{entry}/{slot}.json"),
                serde_json::to_vec(&legacy.envelope).unwrap(),
            )
            .await
            .unwrap();
    }
    let ours = publication_tagged("ci/ours", &["pnpm:v1:linux-arm64-node22-glibc2.17"]);
    let prepared = super::prepare_publication("acme", &ours).unwrap();
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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&stored.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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
    let prepared = super::prepare_publication("acme", &ours).unwrap();

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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&stored.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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
    let prepared = super::prepare_publication("acme", &ours).unwrap();

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
    let owner = super::owner_key("acme", &payload.owner).unwrap();
    let entry = super::entry_digest(&stored.key, &payload.subject);
    let slot = super::compatibility_slot(&payload.compatibility);
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
    let prepared = super::prepare_publication("acme", &ours).unwrap();

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

/// A retried publication of the identical envelope is not an attempt to replace
/// anything, so it stays idempotent rather than becoming a conflict.
#[tokio::test]
async fn republishing_the_same_artifact_stays_idempotent() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    assert!(store.publish("acme", publication("ci/first")).await.unwrap());

    assert!(!store.publish("acme", publication("ci/first")).await.unwrap());
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

fn lookup(owner: &str) -> ResolveArtifactsRequest {
    ResolveArtifactsRequest {
        candidates: vec![ArtifactCandidate {
            key: "dependency-side-effects:v1:deps=abc".to_string(),
            subject: ArtifactSubject::dependency_side_effects(
                PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
                "sha512-source",
            ),
            owner: OwnerScope::organization(owner),
        }],
    }
}

fn publication(builder_id: &str) -> PublishArtifactRequest {
    publication_request("dependency-side-effects:v1:deps=abc", builder_id, None)
}

/// One input key admits one artifact per set of compatibility constraints, so a
/// test wanting several of them for one dependency has to vary the platform —
/// which is the only reason a second artifact for one input is legitimate.
fn for_platform(mut request: PublishArtifactRequest, index: usize) -> PublishArtifactRequest {
    let mut payload: ArtifactPayload =
        serde_json::from_slice(&BASE64.decode(&request.envelope.payload).unwrap()).unwrap();
    // Node major, not the glibc floor: two floors for one architecture and Node
    // major both apply to a consumer meeting the higher one, so they overlap and
    // the second could not be published. Distinct Node majors never share a
    // consumer, which is what a test needing several artifacts at once wants.
    payload.compatibility = CompatibilityConstraints::Tagged {
        tags: vec![format!("pnpm:v1:linux-x64-node{}-glibc2.17", index + 1)],
    };
    request.envelope.payload = BASE64.encode(serde_json::to_vec(&payload).unwrap());
    request
}

fn publication_for_platform(index: usize) -> PublishArtifactRequest {
    for_platform(publication(&format!("ci/{index}")), index)
}

fn publication_tagged(builder_id: &str, tags: &[&str]) -> PublishArtifactRequest {
    let mut request = publication(builder_id);
    let mut payload: ArtifactPayload =
        serde_json::from_slice(&BASE64.decode(&request.envelope.payload).unwrap()).unwrap();
    payload.compatibility = CompatibilityConstraints::Tagged {
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
    };
    request.envelope.payload = BASE64.encode(serde_json::to_vec(&payload).unwrap());
    request
}

fn publication_with_blob(input_key: &str, builder_id: &str) -> PublishArtifactRequest {
    publication_request(input_key, builder_id, Some(b"shared addon"))
}

fn publication_request(
    input_key: &str,
    builder_id: &str,
    blob: Option<&[u8]>,
) -> PublishArtifactRequest {
    let (added, blobs) = match blob {
        Some(bytes) => {
            let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)));
            (
                vec![ArtifactFile {
                    path: "build/addon.node".to_string(),
                    integrity: integrity.clone(),
                    mode: 0o755,
                    size: bytes.len() as u64,
                }],
                vec![ArtifactBlobUpload { integrity, data: BASE64.encode(bytes) }],
            )
        }
        None => (Vec::new(), Vec::new()),
    };
    let payload = ArtifactPayload {
        kind: ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
            "sha512-source",
        ),
        input_key: input_key.to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: builder_id.to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::new(),
        },
        compatibility: CompatibilityConstraints::Universal,
        manifest: ArtifactManifest { added, deleted: Vec::new() },
    };
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    PublishArtifactRequest {
        key: payload.input_key,
        envelope: SignedArtifactEnvelope {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id: "acme-2026".to_string(),
            payload: BASE64.encode(payload_bytes),
            signature: "MAYCAQECAQE=".to_string(),
        },
        blobs,
    }
}

fn workspace_task_publication() -> PublishArtifactRequest {
    let payload = ArtifactPayload {
        kind: WORKSPACE_TASK_ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::workspace_task("packages/app", "build"),
        input_key: "workspace-task:v1:inputs=abc".to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: "ci/linux".to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::new(),
        },
        compatibility: CompatibilityConstraints::Universal,
        manifest: ArtifactManifest { added: Vec::new(), deleted: Vec::new() },
    };
    PublishArtifactRequest {
        key: payload.input_key.clone(),
        envelope: SignedArtifactEnvelope {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id: "acme-2026".to_string(),
            payload: BASE64.encode(serde_json::to_vec(&payload).unwrap()),
            signature: "MAYCAQECAQE=".to_string(),
        },
        blobs: Vec::new(),
    }
}

#[derive(Debug)]
struct FailArtifactWrites {
    inner: InMemory,
    commit_before_error: bool,
    fail_deletes: bool,
    fail_next_quota_write: Option<Arc<AtomicBool>>,
    /// Stands in for the publication that won a race for a slot: the first
    /// creation of this path stores these bytes instead and reports the
    /// conflict the loser would see.
    claim_slot_first: Option<(String, Vec<u8>)>,
    /// Fails reads of the slot *after* the first, so the pre-check still finds
    /// it free and the failure lands on the re-read that follows a lost create
    /// — the only point where the loser is charged for what it did not store.
    fail_slot_read_after_first: Option<Arc<AtomicUsize>>,
    /// Stands in for a publication whose constraints merely overlap this one's.
    /// It lands once this one's variant is written, which is after the overlap
    /// scan found the entry clear — the window a conditional create on a
    /// different path cannot close. Writes pass through rather than failing.
    publish_overlapping_after_create: Option<(String, Vec<u8>)>,
    /// Fails reads of this path, so a scan that reaches it cannot finish.
    fail_reads_of: Option<String>,
    /// Applies the injected write failure to scope markers too. They pass
    /// through by default, so a test injecting a failure reaches the envelope
    /// or blob it is aimed at rather than stopping at the claim.
    fail_scope_writes: bool,
    /// Lets every operation through except the one named, so a test can put a
    /// failure exactly where it means it.
    fail_only: Option<FailOnly>,
    /// Counts writes of the usage document, which is what a reservation and a
    /// release each cost against a hosted store.
    usage_writes: Option<Arc<AtomicUsize>>,
}

#[derive(Debug)]
enum FailOnly {
    /// The quota write that follows this path being stored — the registration a
    /// publication takes again once its artifact is durable, rather than the
    /// reservation that precedes it.
    RegistrationAfter(String),
    DeleteOf(String),
    WriteOf(String),
}

impl fmt::Display for FailArtifactWrites {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fail artifact writes")
    }
}

#[async_trait]
impl ObjectStore for FailArtifactWrites {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        if let Some(writes) = self.usage_writes.as_ref()
            && location.as_ref().ends_with("/quota.json")
        {
            writes.fetch_add(1, Ordering::SeqCst);
        }
        if let Some((slot, winner)) = self.claim_slot_first.as_ref()
            && location.as_ref() == slot
        {
            self.inner
                .put_opts(location, PutPayload::from(winner.clone()), PutOptions::default())
                .await?;
            return Err(object_store::Error::AlreadyExists {
                path: location.to_string(),
                source: std::io::Error::other("slot claimed by another publication").into(),
            });
        }
        if let Some(fail) = self.fail_only.as_ref() {
            let injected = match fail {
                FailOnly::RegistrationAfter(stored) => {
                    location.as_ref().ends_with("/quota.json")
                        && self.inner.head(&ObjectPath::from(stored.as_str())).await.is_ok()
                }
                FailOnly::WriteOf(path) => location.as_ref() == path,
                FailOnly::DeleteOf(_) => false,
            };
            if injected {
                if self.commit_before_error {
                    self.inner.put_opts(location, payload, options).await?;
                }
                return Err(object_store::Error::Generic {
                    store: "test",
                    source: std::io::Error::other("injected write failure").into(),
                });
            }
            return self.inner.put_opts(location, payload, options).await;
        }
        if !self.fail_scope_writes && location.as_ref().contains("/scopes/") {
            return self.inner.put_opts(location, payload, options).await;
        }
        if location.as_ref().ends_with("/quota.json") {
            if self
                .fail_next_quota_write
                .as_ref()
                .is_some_and(|fail| fail.swap(false, Ordering::SeqCst))
            {
                return Err(object_store::Error::Generic {
                    store: "test",
                    source: std::io::Error::other("injected quota write failure").into(),
                });
            }
            self.inner.put_opts(location, payload, options).await
        } else if let Some((path, envelope)) = self.publish_overlapping_after_create.as_ref() {
            let stored = self.inner.put_opts(location, payload, options).await?;
            if location.as_ref() != path {
                self.inner
                    .put_opts(
                        &ObjectPath::from(path.as_str()),
                        PutPayload::from(envelope.clone()),
                        PutOptions::default(),
                    )
                    .await?;
            }
            Ok(stored)
        } else {
            if self.commit_before_error {
                self.inner.put_opts(location, payload, options).await?;
            }
            Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected artifact write failure").into(),
            })
        }
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, options).await
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        if self.fail_reads_of.as_ref().is_some_and(|path| location.as_ref() == path) {
            return Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected variant read failure").into(),
            });
        }
        if let Some(reads) = self.fail_slot_read_after_first.as_ref()
            && self.claim_slot_first.as_ref().is_some_and(|(slot, _)| location.as_ref() == slot)
            && reads.fetch_add(1, Ordering::SeqCst) > 0
        {
            return Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected slot read failure").into(),
            });
        }
        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<ObjectPath>>,
    ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
        if let Some(FailOnly::DeleteOf(failing)) = self.fail_only.as_ref() {
            let failing = failing.clone();
            let inner = self.inner.clone();
            return locations
                .then(move |location| {
                    let (failing, inner) = (failing.clone(), inner.clone());
                    async move {
                        let location = location?;
                        if location.as_ref() == failing {
                            return Err(object_store::Error::Generic {
                                store: "test",
                                source: std::io::Error::other("injected deletion failure").into(),
                            });
                        }
                        inner.delete(&location).await?;
                        Ok(location)
                    }
                })
                .boxed();
        }
        if self.fail_deletes {
            locations
                .map(|location| {
                    location?;
                    Err(object_store::Error::Generic {
                        store: "test",
                        source: std::io::Error::other("injected artifact deletion failure").into(),
                    })
                })
                .boxed()
        } else {
            self.inner.delete_stream(locations)
        }
    }

    fn list(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&ObjectPath>,
        offset: &ObjectPath,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list_with_offset(prefix, offset)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
        options: CopyOptions,
    ) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }

    async fn rename_opts(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
        options: RenameOptions,
    ) -> object_store::Result<()> {
        self.inner.rename_opts(from, to, options).await
    }
}
