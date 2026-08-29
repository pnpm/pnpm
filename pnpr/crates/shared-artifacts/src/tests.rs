use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
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
    // never charged. What the loser stored is the marker claiming the scope it
    // reaches — its envelope never landed — and it holds that until reclamation
    // finds no artifact of its own behind it.
    assert_eq!(
        usage.global_bytes,
        publication("ci/loser").envelope.digest().unwrap().len() as u64,
        "the loser is charged for the scope it claimed and nothing else",
    );
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
