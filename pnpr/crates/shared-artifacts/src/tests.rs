use std::{collections::BTreeMap, fmt, sync::Arc};

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
    ArtifactManifest, ArtifactPayload, ArtifactVariant, BuilderProfile, CompatibilityConstraints,
    MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE, OwnerScope, PackageIdentity,
    PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact,
    SIGNATURE_ALGORITHM, SignedArtifactEnvelope,
};
use pnpr_config::{HostedStoreConfig, normalize_key_prefix};
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
                publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/first"),
            )
            .await
            .unwrap(),
    );
    assert!(
        second
            .publish(
                "acme",
                publication_with_blob("dependency-side-effects:v1:deps=abc", "ci/second"),
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
            store.publish("acme", publication(&format!("ci/{index}"))).await
        }));
    }
    for publication in publications {
        assert!(publication.await.unwrap().unwrap());
    }

    let usage_path = object_store::path::Path::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    let expected = (0..PUBLICATIONS)
        .map(|index| {
            serde_json::to_vec(&publication(&format!("ci/{index}")).envelope).unwrap().len()
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
    let expected =
        b"shared addon".len() as u64 + serde_json::to_vec(&request.envelope).unwrap().len() as u64;

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
    });
    let config =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&backend), prefix: String::new() };
    let scratch = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&config, scratch.path()).unwrap();
    let request = publication("ci/ambiguous-envelope");
    let expected_usage = serde_json::to_vec(&request.envelope).unwrap().len() as u64;

    store.publish("acme", request).await.unwrap_err();

    let usage_path = ObjectPath::from(".pnpr-artifacts/v0/quota.json");
    let usage: ArtifactUsage =
        serde_json::from_slice(&backend.get(&usage_path).await.unwrap().bytes().await.unwrap())
            .unwrap();
    assert_eq!(usage.global_bytes, expected_usage);
    assert_eq!(usage.owner_bytes.values().copied().sum::<u64>(), expected_usage);
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
    let backend: Arc<dyn ObjectStore> =
        Arc::new(FailArtifactWrites { inner, commit_before_error: false, fail_deletes: true });
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

#[tokio::test]
async fn the_variant_limit_is_applied_at_read_time() {
    let storage = TempDir::new().unwrap();
    let store = SharedArtifactStore::new(&HostedStoreConfig::Fs, storage.path()).unwrap();
    for index in 0..MAX_VARIANTS_PER_CANDIDATE + 2 {
        assert!(store.publish("acme", publication(&format!("ci/{index}"))).await.unwrap());
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
            package: PackageIdentity {
                name: "native-addon".to_string(),
                version: "1.0.0".to_string(),
            },
            source_integrity: "sha512-source".to_string(),
            owner: OwnerScope::organization(owner),
        }],
    }
}

fn publication(builder_id: &str) -> PublishArtifactRequest {
    publication_request("dependency-side-effects:v1:deps=abc", builder_id, None)
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
        package: PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
        source_integrity: "sha512-source".to_string(),
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

#[derive(Debug)]
struct FailArtifactWrites {
    inner: InMemory,
    commit_before_error: bool,
    fail_deletes: bool,
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
        if location.as_ref().ends_with("/quota.json") {
            self.inner.put_opts(location, payload, options).await
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
