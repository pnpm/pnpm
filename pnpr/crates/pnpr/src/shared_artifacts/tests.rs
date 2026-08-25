use std::{collections::BTreeMap, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_shared_artifact_protocol::{
    ARTIFACT_KIND, ArtifactManifest, ArtifactPayload, ArtifactVariant, BuilderProfile,
    CompatibilityConstraints, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE, OwnerScope,
    PackageIdentity, PublishArtifactRequest, ResolveArtifactsResponse, ResolvedArtifact,
    SIGNATURE_ALGORITHM, SignedArtifactEnvelope,
};
use tempfile::TempDir;
use tokio::{fs, sync::Barrier};

use super::{
    ResolveBudget, acquire_publication_lock, artifact_usage_path, is_variant_file,
    pending_usage_file, publish, reserve_storage_quota_with_limits,
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
    assert!(is_variant_file(format!("{digest}.json").as_ref()));
    assert!(!is_variant_file(format!("{digest}.json.tmp").as_ref()));
    assert!(!is_variant_file(format!("{}.json", "A".repeat(64)).as_ref()));
}

#[tokio::test]
async fn publication_storage_is_bounded_per_owner_and_globally() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let owner = root.join("owner");
    let other_owner = root.join("other-owner");
    fs::create_dir_all(&owner).await.unwrap();
    fs::create_dir_all(&other_owner).await.unwrap();
    fs::create_dir_all(root.join(".locks")).await.unwrap();
    fs::write(owner.join("entry"), b"123456").await.unwrap();
    fs::write(other_owner.join("entry"), b"123").await.unwrap();
    fs::write(root.join(".locks/lock-state"), vec![0; 100]).await.unwrap();

    let pending = pending_usage_file(&root, &owner.join("new-entry"), 4).unwrap();
    let usage =
        reserve_storage_quota_with_limits(&root, &owner, vec![pending], 10, 13).await.unwrap();
    assert_eq!(usage.global_bytes, 13);
    assert_eq!(usage.owner_bytes.values().copied().sum::<u64>(), 13);

    let too_large_for_owner = pending_usage_file(&root, &owner.join("owner-overflow"), 5).unwrap();
    assert!(
        reserve_storage_quota_with_limits(&root, &owner, vec![too_large_for_owner], 10, 20)
            .await
            .is_err(),
    );
    let too_large_globally = pending_usage_file(&root, &owner.join("global-overflow"), 4).unwrap();
    assert!(
        reserve_storage_quota_with_limits(&root, &owner, vec![too_large_globally], 20, 12)
            .await
            .is_err(),
    );
}

#[tokio::test]
async fn an_unlocked_publication_lock_file_is_reused() {
    let storage = TempDir::new().unwrap();
    let path = storage.path().join("publication.lock");
    fs::write(&path, b"residue").await.unwrap();

    let lock = acquire_publication_lock(path.clone()).await.unwrap();
    assert!(path.is_file());
    drop(lock);

    acquire_publication_lock(path).await.unwrap();
}

#[tokio::test]
async fn duplicate_publication_does_not_rewrite_usage() {
    let storage = TempDir::new().unwrap();
    let request = publication("ci/duplicate");

    assert!(publish(storage.path(), "acme", request.clone()).await.unwrap());
    let usage_path = artifact_usage_path(&storage.path().join("shared-artifacts/v0"));
    let before = fs::read(&usage_path).await.unwrap();

    assert!(!publish(storage.path(), "acme", request).await.unwrap());
    assert_eq!(fs::read(usage_path).await.unwrap(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn concurrent_publishers_sharing_a_cache_respect_the_variant_limit() {
    const PUBLICATIONS: usize = 16;

    let storage = TempDir::new().unwrap();
    let barrier = Arc::new(Barrier::new(PUBLICATIONS + 1));
    let mut publications = Vec::with_capacity(PUBLICATIONS);
    for index in 0..PUBLICATIONS {
        let barrier = Arc::clone(&barrier);
        let cache_storage = storage.path().to_path_buf();
        publications.push(tokio::spawn(async move {
            barrier.wait().await;
            publish(&cache_storage, "acme", publication(&format!("ci/concurrent/{index}")))
                .await
                .map_err(|err| err.to_string())
        }));
    }
    barrier.wait().await;

    let mut accepted = 0;
    let mut rejected = Vec::new();
    for publication in publications {
        match publication.await.unwrap() {
            Ok(true) => accepted += 1,
            Ok(false) => panic!("all publications have distinct envelopes"),
            Err(err) => rejected.push(err),
        }
    }
    assert_eq!(accepted, MAX_VARIANTS_PER_CANDIDATE);
    assert_eq!(rejected.len(), PUBLICATIONS - accepted);
    assert!(rejected.iter().all(|error| error.contains("variant limit")));
}

fn publication(builder_id: &str) -> PublishArtifactRequest {
    let payload = ArtifactPayload {
        kind: ARTIFACT_KIND.to_string(),
        package: PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
        source_integrity: "sha512-source".to_string(),
        input_key: "dependency-side-effects:v1:deps=abc".to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: builder_id.to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::new(),
        },
        compatibility: CompatibilityConstraints::Universal,
        manifest: ArtifactManifest { added: Vec::new(), deleted: Vec::new() },
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
        blobs: Vec::new(),
    }
}
