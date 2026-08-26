use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::Path,
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_shared_artifact_protocol::{
    ARTIFACT_KIND, ArtifactBlobUpload, ArtifactFile, ArtifactManifest, ArtifactPayload,
    ArtifactVariant, BuilderProfile, CompatibilityConstraints, MAX_RESOLVE_RESPONSE_SIZE,
    MAX_VARIANTS_PER_CANDIDATE, OwnerScope, PackageIdentity, PublishArtifactRequest,
    ResolveArtifactsResponse, ResolvedArtifact, SIGNATURE_ALGORITHM, SignedArtifactEnvelope,
    blob_id,
};
use sha2::{Digest as _, Sha512};
use tempfile::TempDir;
use tokio::{fs, sync::Barrier, time::timeout};

use super::{
    ArtifactRecoveryLocks, ArtifactUsage, BLOB_LOCK_STRIPES, PendingUsage, PendingUsageFile,
    PendingUsageLock, ResolveBudget, StorageQuotaReservation, acquire_artifact_lock,
    artifact_usage_path, blob_lock_key, blob_lock_path_for_key, entry_digest, entry_lock_path,
    is_variant_file, load_artifact_usage, owner_dir, owner_lock_path, owner_usage_key,
    pending_usage_file, publish, reconcile_storage_reservations,
    reserve_storage_quota_with_locks_and_limits, stored_bytes, try_acquire_artifact_lock,
    write_artifact_usage,
};
use crate::{error::Result, storage::unique_tmp_path};

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

#[test]
fn blob_lock_file_names_are_bounded() {
    let root = Path::new("shared-artifacts/v0");
    let paths: HashSet<_> = (0..10_000)
        .map(|index| {
            blob_lock_path_for_key(root, &blob_lock_key("owner", &format!("{index:0128x}")))
        })
        .collect();
    assert!(paths.len() <= BLOB_LOCK_STRIPES);
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

    let owner_key = owner_usage_key(&owner).unwrap();
    let pending = pending_usage_file(&root, &owner.join("new-entry"), 4).unwrap();
    reserve_storage_quota_with_limits(
        &root,
        "reservation",
        &owner_key,
        "entry",
        vec![pending],
        10,
        13,
    )
    .await
    .unwrap();

    let too_large_for_owner = pending_usage_file(&root, &owner.join("owner-overflow"), 5).unwrap();
    assert!(
        reserve_storage_quota_with_limits(
            &root,
            "owner-overflow",
            &owner_key,
            "entry",
            vec![too_large_for_owner],
            10,
            20,
        )
        .await
        .is_err(),
    );
    let too_large_globally = pending_usage_file(&root, &owner.join("global-overflow"), 4).unwrap();
    assert!(
        reserve_storage_quota_with_limits(
            &root,
            "global-overflow",
            &owner_key,
            "entry",
            vec![too_large_globally],
            20,
            12,
        )
        .await
        .is_err(),
    );
}

#[tokio::test]
async fn legacy_pending_usage_is_reconciled() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    fs::create_dir_all(root.join(".locks")).await.unwrap();
    fs::write(
        artifact_usage_path(&root),
        br#"{"global_bytes":4,"owner_bytes":{"owner":4},"pending":{"owner":"owner","files":[{"path":"owner/missing","size":4}]}}"#,
    )
    .await
    .unwrap();

    reserve_storage_quota_with_limits(&root, "reservation", "owner", "entry", Vec::new(), 10, 10)
        .await
        .unwrap();
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert_eq!(usage.owner_bytes.get("owner"), Some(&0));
}

#[tokio::test]
async fn owner_scoped_pending_usage_is_reconciled() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    fs::create_dir_all(root.join(".locks")).await.unwrap();
    fs::write(
        artifact_usage_path(&root),
        br#"{"global_bytes":4,"owner_bytes":{"owner":4},"pending":{"owner":[{"path":"owner/missing","size":4}]}}"#,
    )
    .await
    .unwrap();

    reserve_storage_quota_with_limits(&root, "reservation", "owner", "entry", Vec::new(), 10, 10)
        .await
        .unwrap();
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert_eq!(usage.owner_bytes.get("owner"), Some(&0));
}

#[tokio::test]
async fn orphaned_atomic_temps_are_removed_before_releasing_quota() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let final_path = root.join("owner/missing");
    fs::create_dir_all(final_path.parent().unwrap()).await.unwrap();
    fs::create_dir_all(root.join(".locks")).await.unwrap();
    let temp_path = unique_tmp_path(&final_path);
    fs::write(&temp_path, b"1234").await.unwrap();
    fs::write(
        artifact_usage_path(&root),
        br#"{"global_bytes":4,"owner_bytes":{"owner":4},"pending":{"crashed":{"owner":"owner","lock":{"type":"entry","entry":"entry"},"files":[{"path":"owner/missing","size":4}]}}}"#,
    )
    .await
    .unwrap();
    let _entry_lock =
        acquire_artifact_lock(entry_lock_path(&root, "owner", "entry")).await.unwrap();

    reserve_storage_quota_with_limits(&root, "reservation", "owner", "entry", Vec::new(), 10, 10)
        .await
        .unwrap();

    assert!(!fs::try_exists(temp_path).await.unwrap());
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert_eq!(usage.owner_bytes.get("owner"), Some(&0));
}

#[tokio::test]
async fn an_incomplete_publication_removes_its_blobs_and_releases_all_quota() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let blob_path = root.join("owner/blobs/blob");
    fs::create_dir_all(blob_path.parent().unwrap()).await.unwrap();
    fs::create_dir_all(root.join(".locks")).await.unwrap();
    fs::write(&blob_path, b"blob").await.unwrap();
    fs::write(
        artifact_usage_path(&root),
        br#"{"global_bytes":12,"owner_bytes":{"owner":12},"pending":{"crashed":{"owner":"owner","lock":{"type":"owner"},"commit_file":"owner/entries/entry/variant.json","files":[{"path":"owner/blobs/blob","size":4},{"path":"owner/entries/entry/variant.json","size":8}]}}}"#,
    )
    .await
    .unwrap();
    let _owner_lock = acquire_artifact_lock(owner_lock_path(&root, "owner")).await.unwrap();
    let empty = BTreeSet::new();

    reconcile_storage_reservations(
        &root,
        &ArtifactRecoveryLocks {
            owner: "owner",
            publication: &PendingUsageLock::Entry { entry: "current".to_string() },
            owner_locked: true,
            blob_locks: &empty,
            preserved_blob_ids: &empty,
        },
    )
    .await
    .unwrap();

    assert!(!fs::try_exists(blob_path).await.unwrap());
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 0);
    assert_eq!(usage.owner_bytes.get("owner"), Some(&0));
    assert!(usage.pending.is_empty());
}

#[tokio::test]
async fn active_blob_writes_are_not_cleaned_up_by_reconciliation() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let final_path = root.join("owner/blobs/blob");
    fs::create_dir_all(final_path.parent().unwrap()).await.unwrap();
    fs::create_dir_all(root.join(".locks")).await.unwrap();
    let temp_path = unique_tmp_path(&final_path);
    fs::write(&temp_path, b"1234").await.unwrap();
    fs::write(
        artifact_usage_path(&root),
        br#"{"global_bytes":4,"owner_bytes":{"owner":4},"pending":{"crashed":{"owner":"owner","lock":{"type":"entry","entry":"first-entry"},"files":[{"path":"owner/blobs/blob","size":4}]}}}"#,
    )
    .await
    .unwrap();
    let blob_lock =
        acquire_artifact_lock(blob_lock_path_for_key(&root, &blob_lock_key("owner", "blob")))
            .await
            .unwrap();

    reserve_storage_quota_with_limits(
        &root,
        "reservation",
        "owner",
        "second-entry",
        Vec::new(),
        10,
        10,
    )
    .await
    .unwrap();

    assert!(fs::try_exists(&temp_path).await.unwrap());
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 4);
    assert!(usage.pending.contains_key("crashed"));

    drop(blob_lock);
    reserve_storage_quota_with_limits(
        &root,
        "next-reservation",
        "owner",
        "third-entry",
        Vec::new(),
        10,
        10,
    )
    .await
    .unwrap();
    assert!(!fs::try_exists(temp_path).await.unwrap());
    assert_eq!(load_artifact_usage(&root).await.unwrap().global_bytes, 0);
}

#[tokio::test]
async fn blob_classification_waits_for_complete_rollback_and_preserves_required_blobs() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let owner = "owner";
    let first_blob = "first-blob";
    let first_lock = blob_lock_key(owner, first_blob);
    let second_blob = (0_u64..1024)
        .map(|index| format!("second-blob-{index}"))
        .find(|blob| blob_lock_key(owner, blob) != first_lock)
        .unwrap();
    let second_lock = blob_lock_key(owner, &second_blob);
    let blobs_dir = root.join(owner).join("blobs");
    fs::create_dir_all(&blobs_dir).await.unwrap();
    let first_path = blobs_dir.join(first_blob);
    let second_path = blobs_dir.join(&second_blob);
    fs::write(&first_path, b"first").await.unwrap();
    fs::write(&second_path, b"second").await.unwrap();
    let variant_path = root.join(owner).join("entries/crashed/variant.json");
    let variant_file = pending_usage_file(&root, &variant_path, 1).unwrap();
    let commit_file = variant_file.path.clone();
    write_artifact_usage(
        &root,
        &ArtifactUsage {
            global_bytes: 12,
            owner_bytes: BTreeMap::from([(owner.to_string(), 12)]),
            pending: BTreeMap::from([(
                "crashed".to_string(),
                PendingUsage {
                    owner: owner.to_string(),
                    lock: PendingUsageLock::Entry { entry: "crashed".to_string() },
                    commit_file: Some(commit_file),
                    files: vec![
                        pending_usage_file(&root, &first_path, 5).unwrap(),
                        pending_usage_file(&root, &second_path, 6).unwrap(),
                        variant_file,
                    ],
                },
            )]),
        },
    )
    .await
    .unwrap();
    let current_lock = PendingUsageLock::Entry { entry: "current".to_string() };
    let _current_entry =
        acquire_artifact_lock(entry_lock_path(&root, owner, "current")).await.unwrap();
    let _first_blob =
        acquire_artifact_lock(blob_lock_path_for_key(&root, &first_lock)).await.unwrap();
    let second_blob_guard =
        acquire_artifact_lock(blob_lock_path_for_key(&root, &second_lock)).await.unwrap();
    let locked_blob_locks = BTreeSet::from([first_lock]);
    let preserved_blob_ids = BTreeSet::from([first_blob.to_string()]);
    let recovery_locks = ArtifactRecoveryLocks {
        owner,
        publication: &current_lock,
        owner_locked: false,
        blob_locks: &locked_blob_locks,
        preserved_blob_ids: &preserved_blob_ids,
    };

    assert!(!reconcile_storage_reservations(&root, &recovery_locks).await.unwrap());
    assert!(fs::try_exists(&first_path).await.unwrap());
    assert!(fs::try_exists(&second_path).await.unwrap());
    assert!(load_artifact_usage(&root).await.unwrap().pending.contains_key("crashed"));

    drop(second_blob_guard);
    assert!(reconcile_storage_reservations(&root, &recovery_locks).await.unwrap());
    assert!(fs::try_exists(first_path).await.unwrap());
    assert!(!fs::try_exists(second_path).await.unwrap());
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 5);
    assert_eq!(usage.owner_bytes.get(owner), Some(&5));
    assert!(usage.pending.is_empty());
}

#[tokio::test]
async fn publication_reuses_a_blob_preserved_from_a_stale_transaction() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let artifact_owner =
        owner_dir(storage.path(), "acme", &OwnerScope::organization("acme")).unwrap();
    let owner = owner_usage_key(&artifact_owner).unwrap();
    let blob_bytes = b"recovered shared blob";
    let mut request = publication_request(
        "acme",
        "dependency-side-effects:v1:recovered",
        "ci/recovered",
        Some(blob_bytes),
    );
    let blob = blob_id(&request.blobs[0].integrity).unwrap();
    request.blobs.clear();
    let blob_path = artifact_owner.join("blobs").join(&blob);
    fs::create_dir_all(blob_path.parent().unwrap()).await.unwrap();
    fs::write(&blob_path, blob_bytes).await.unwrap();
    let variant_path = artifact_owner.join("entries/crashed/variant.json");
    let variant_file = pending_usage_file(&root, &variant_path, 1).unwrap();
    let commit_file = variant_file.path.clone();
    write_artifact_usage(
        &root,
        &ArtifactUsage {
            global_bytes: blob_bytes.len() as u64 + 1,
            owner_bytes: BTreeMap::from([(owner.clone(), blob_bytes.len() as u64 + 1)]),
            pending: BTreeMap::from([(
                "crashed".to_string(),
                PendingUsage {
                    owner: owner.clone(),
                    lock: PendingUsageLock::Entry { entry: "crashed".to_string() },
                    commit_file: Some(commit_file),
                    files: vec![
                        pending_usage_file(&root, &blob_path, blob_bytes.len() as u64).unwrap(),
                        variant_file,
                    ],
                },
            )]),
        },
    )
    .await
    .unwrap();

    assert!(publish(storage.path(), "acme", request).await.unwrap());
    assert_eq!(fs::read(blob_path).await.unwrap(), blob_bytes);
    let usage = load_artifact_usage(&root).await.unwrap();
    let actual_bytes = stored_bytes(&artifact_owner, None).await.unwrap();
    assert_eq!(usage.global_bytes, actual_bytes);
    assert_eq!(usage.owner_bytes.get(&owner), Some(&actual_bytes));
    assert!(usage.pending.is_empty());
}

#[tokio::test]
async fn active_entry_reservations_are_not_reconciled() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let owner = "active-owner";
    let entry = "active-entry";
    let _entry_lock = acquire_artifact_lock(entry_lock_path(&root, owner, entry)).await.unwrap();
    let pending = pending_usage_file(&root, &root.join(owner).join("pending"), 4).unwrap();
    reserve_storage_quota_with_limits(
        &root,
        "active-reservation",
        owner,
        entry,
        vec![pending],
        10,
        10,
    )
    .await
    .unwrap();

    reserve_storage_quota_with_limits(
        &root,
        "other-reservation",
        owner,
        "other-entry",
        Vec::new(),
        10,
        10,
    )
    .await
    .unwrap();
    let usage = load_artifact_usage(&root).await.unwrap();
    assert_eq!(usage.global_bytes, 4);
    assert!(usage.pending.contains_key("active-reservation"));
}

async fn reserve_storage_quota_with_limits(
    artifact_root: &Path,
    reservation: &str,
    owner: &str,
    entry: &str,
    files: Vec<PendingUsageFile>,
    owner_limit: u64,
    global_limit: u64,
) -> Result<()> {
    let locked_blob_locks = BTreeSet::new();
    reserve_storage_quota_with_locks_and_limits(
        artifact_root,
        StorageQuotaReservation {
            id: reservation,
            owner,
            lock: PendingUsageLock::Entry { entry: entry.to_string() },
            commit_file: None,
            locked_blob_locks: &locked_blob_locks,
            files,
        },
        owner_limit,
        global_limit,
    )
    .await
}

#[tokio::test]
async fn an_unlocked_publication_lock_file_is_reused() {
    let storage = TempDir::new().unwrap();
    let path = storage.path().join("publication.lock");
    fs::write(&path, b"residue").await.unwrap();

    let lock = acquire_artifact_lock(path.clone()).await.unwrap();
    assert!(path.is_file());
    drop(lock);

    acquire_artifact_lock(path).await.unwrap();
}

#[tokio::test]
async fn an_entry_lock_does_not_block_another_entry() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let acme_dir = owner_dir(storage.path(), "acme", &OwnerScope::organization("acme")).unwrap();
    let acme_owner = owner_usage_key(&acme_dir).unwrap();
    let _acme_lock = acquire_artifact_lock(entry_lock_path(&root, &acme_owner, "unrelated-entry"))
        .await
        .unwrap();

    let published = timeout(
        std::time::Duration::from_secs(5),
        publish(storage.path(), "acme", publication_for("acme", "ci/other-entry")),
    )
    .await
    .expect("an unrelated entry must not wait for the held entry lock")
    .unwrap();
    assert!(published);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_entry_does_not_block_an_unrelated_entry_for_the_same_owner() {
    let storage = TempDir::new().unwrap();
    let root = storage.path().join("shared-artifacts/v0");
    let acme_dir = owner_dir(storage.path(), "acme", &OwnerScope::organization("acme")).unwrap();
    let acme_owner = owner_usage_key(&acme_dir).unwrap();
    let blocked = publication_request(
        "acme",
        "dependency-side-effects:v1:blocked",
        "ci/blocked",
        Some(b"blocked blob"),
    );
    let blocked_payload = blocked.validate().unwrap().payload;
    let blocked_entry =
        entry_digest(&blocked.key, &blocked_payload.package, &blocked_payload.source_integrity);
    let blocked_blob = blob_id(&blocked.blobs[0].integrity).unwrap();
    let blocked_blob_lock = blob_lock_key(&acme_owner, &blocked_blob);
    let blob_blocker =
        acquire_artifact_lock(blob_lock_path_for_key(&root, &blocked_blob_lock)).await.unwrap();
    let cache_storage = storage.path().to_path_buf();
    let blocked_publication =
        tokio::spawn(async move { publish(&cache_storage, "acme", blocked).await });
    timeout(std::time::Duration::from_secs(5), async {
        loop {
            if try_acquire_artifact_lock(entry_lock_path(&root, &acme_owner, &blocked_entry))
                .await
                .unwrap()
                .is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the blocked publication must acquire only its entry lock before waiting");

    let unrelated = (0_u64..)
        .find_map(|index| {
            let bytes = index.to_le_bytes();
            let request = publication_request(
                "acme",
                "dependency-side-effects:v1:unrelated",
                "ci/unrelated",
                Some(&bytes),
            );
            let blob = blob_id(&request.blobs[0].integrity).unwrap();
            (blob_lock_key(&acme_owner, &blob) != blocked_blob_lock).then_some(request)
        })
        .unwrap();
    let published =
        timeout(std::time::Duration::from_secs(5), publish(storage.path(), "acme", unrelated))
            .await
            .expect("an unrelated entry must not wait for the stalled publication")
            .unwrap();
    assert!(published);

    drop(blob_blocker);
    assert!(blocked_publication.await.unwrap().unwrap());
}

#[tokio::test]
async fn rejected_missing_blobs_do_not_create_lock_files() {
    let storage = TempDir::new().unwrap();
    for index in 0..64_u64 {
        let bytes = index.to_le_bytes();
        let mut request = publication_request(
            "acme",
            &format!("dependency-side-effects:v1:missing={index}"),
            &format!("ci/missing/{index}"),
            Some(&bytes),
        );
        request.blobs.clear();
        assert!(publish(storage.path(), "acme", request).await.is_err());
    }

    let locks = storage.path().join("shared-artifacts/v0/.locks");
    assert!(!fs::try_exists(locks).await.unwrap());
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

#[tokio::test]
async fn concurrent_entries_count_a_shared_blob_once() {
    let storage = TempDir::new().unwrap();
    let first = publish(
        storage.path(),
        "acme",
        publication_with_blob("dependency-side-effects:v1:first", "ci/first"),
    );
    let second = publish(
        storage.path(),
        "acme",
        publication_with_blob("dependency-side-effects:v1:second", "ci/second"),
    );
    let (first, second) = tokio::join!(first, second);
    assert!(first.unwrap());
    assert!(second.unwrap());

    let root = storage.path().join("shared-artifacts/v0");
    let owner_dir = owner_dir(storage.path(), "acme", &OwnerScope::organization("acme")).unwrap();
    let owner = owner_usage_key(&owner_dir).unwrap();
    let usage = load_artifact_usage(&root).await.unwrap();
    let actual_bytes = stored_bytes(&owner_dir, None).await.unwrap();
    assert_eq!(usage.global_bytes, actual_bytes);
    assert_eq!(usage.owner_bytes.get(&owner), Some(&actual_bytes));
    assert!(usage.pending.is_empty());
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

#[tokio::test]
async fn a_variant_limit_rejection_does_not_store_or_charge_uploaded_blobs() {
    let storage = TempDir::new().unwrap();
    for index in 0..MAX_VARIANTS_PER_CANDIDATE {
        assert!(
            publish(storage.path(), "acme", publication(&format!("ci/accepted/{index}")))
                .await
                .unwrap(),
        );
    }
    let root = storage.path().join("shared-artifacts/v0");
    let usage_before = fs::read(artifact_usage_path(&root)).await.unwrap();
    let rejected = publication_request(
        "acme",
        "dependency-side-effects:v1:deps=abc",
        "ci/rejected",
        Some(b"rejected blob"),
    );
    let rejected_blob = blob_id(&rejected.blobs[0].integrity).unwrap();
    let owner = owner_dir(storage.path(), "acme", &OwnerScope::organization("acme")).unwrap();

    let error = publish(storage.path(), "acme", rejected).await.unwrap_err();

    assert!(error.to_string().contains("variant limit"));
    assert!(!fs::try_exists(owner.join("blobs").join(rejected_blob)).await.unwrap());
    assert_eq!(fs::read(artifact_usage_path(&root)).await.unwrap(), usage_before);
}

fn publication(builder_id: &str) -> PublishArtifactRequest {
    publication_for("acme", builder_id)
}

fn publication_for(owner: &str, builder_id: &str) -> PublishArtifactRequest {
    publication_request(owner, "dependency-side-effects:v1:deps=abc", builder_id, None)
}

fn publication_with_blob(input_key: &str, builder_id: &str) -> PublishArtifactRequest {
    publication_request("acme", input_key, builder_id, Some(b"shared addon"))
}

fn publication_request(
    owner: &str,
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
        owner: OwnerScope::organization(owner),
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
