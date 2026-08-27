use std::{collections::BTreeMap, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use object_store::{ObjectStore, ObjectStoreExt, memory::InMemory};
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

use super::{ArtifactUsage, ResolveBudget, SharedArtifactStore, is_variant_file};

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

    let bytes = store
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
