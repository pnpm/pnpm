use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{File, OpenOptions, TryLockError},
    io::{ErrorKind, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactProtocolError,
    ArtifactVariant, MAX_CANDIDATES, MAX_FILE_SIZE, MAX_RESOLVE_RESPONSE_SIZE,
    MAX_VARIANTS_PER_CANDIDATE, OwnerScope, PackageIdentity, PublishArtifactRequest,
    ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact, SignedArtifactEnvelope,
    blob_id, verify_blob,
};
use sha2::{Digest as _, Sha256, Sha512};
use tokio::{
    fs,
    io::{AsyncReadExt as _, AsyncSeekExt as _},
    time::sleep,
};

use crate::storage::{remove_atomic_write_temps, write_atomic};
use pnpr_error::{RegistryError, Result};

const ARTIFACT_CACHE_DIR: &str = "shared-artifacts/v0";
const ARTIFACT_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const ARTIFACT_USAGE_FILE: &str = "usage.json";
const ARTIFACT_BLOB_READ_CHUNK: usize = 64 * 1024;
const BLOB_LOCK_STRIPES: usize = 256;
const MAX_OWNER_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_GLOBAL_ARTIFACT_BYTES: u64 = 10 * MAX_OWNER_ARTIFACT_BYTES;
static ARTIFACT_RESERVATION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct ArtifactUsage {
    global_bytes: u64,
    owner_bytes: BTreeMap<String, u64>,
    #[serde(default, deserialize_with = "deserialize_pending_usage")]
    pending: BTreeMap<String, PendingUsage>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PendingUsage {
    owner: String,
    lock: PendingUsageLock,
    /// Recovery treats the reservation as committed when this file has its reserved size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit_file: Option<String>,
    files: Vec<PendingUsageFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum PendingUsageLock {
    Entry { entry: String },
    Owner,
    Global,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyPendingUsage {
    owner: String,
    files: Vec<PendingUsageFile>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PendingUsageFile {
    path: String,
    size: u64,
}

struct StorageQuotaReservation<'a> {
    id: &'a str,
    owner: &'a str,
    lock: PendingUsageLock,
    commit_file: Option<String>,
    locked_blob_locks: &'a BTreeSet<String>,
    files: Vec<PendingUsageFile>,
}

#[derive(Default)]
struct PendingUsageReconciliation {
    usage_changed: bool,
    blocked_locked_blobs: bool,
}

struct ArtifactRecoveryLocks<'a> {
    owner: &'a str,
    publication: &'a PendingUsageLock,
    active_reservation: Option<&'a str>,
    adoption_commit_file: Option<&'a str>,
    owner_locked: bool,
    blob_locks: &'a BTreeSet<String>,
    preserved_blob_ids: &'a BTreeSet<String>,
}

pub(crate) fn parse_publish(body: &[u8]) -> Result<PublishArtifactRequest> {
    serde_json::from_slice(body)
        .map_err(|err| bad_request(format!("invalid shared artifact request: {err}")))
}

pub(crate) async fn publish(
    cache_storage: &Path,
    username: &str,
    request: PublishArtifactRequest,
) -> Result<bool> {
    let validated = request.validate().map_err(|err| protocol_error(&err))?;
    let payload = validated.payload;
    let mut uploads = validated.blobs;
    let artifact_root = cache_storage.join(ARTIFACT_CACHE_DIR);
    let owner_dir = owner_dir(cache_storage, username, &payload.owner)?;
    let owner = owner_usage_key(&owner_dir)?;
    let entry_digest = entry_digest(&request.key, &payload.package, &payload.source_integrity);
    let envelope_digest = request.envelope.digest().map_err(|err| protocol_error(&err))?;
    let required: BTreeMap<&str, u64> =
        payload.manifest.added.iter().map(|file| (file.integrity.as_str(), file.size)).collect();
    let blobs_dir = owner_dir.join("blobs");
    let mut required_by_lock = BTreeMap::<String, Vec<(&str, String, u64)>>::new();
    for (integrity, size) in required {
        let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
        required_by_lock.entry(blob_lock_key(&owner, &id)).or_default().push((integrity, id, size));
    }
    for blobs in required_by_lock.values() {
        for (integrity, id, size) in blobs {
            let Some(bytes) = uploads.get(*integrity) else {
                if !fs::try_exists(blobs_dir.join(id)).await? {
                    return Err(bad_request(format!(
                        "signed manifest references blob {id} without uploading it",
                    )));
                }
                continue;
            };
            if bytes.len() as u64 != *size {
                return Err(bad_request(format!(
                    "blob {id} has {} bytes but the signed manifest declares {}",
                    bytes.len(),
                    size,
                )));
            }
            verify_blob(integrity, bytes).map_err(|err| protocol_error(&err))?;
        }
    }

    let reservation = reservation_id(&owner, &entry_digest, &envelope_digest);
    let publication_lock = PendingUsageLock::Entry { entry: entry_digest.clone() };
    let key_dir = owner_dir.join("entries").join(&entry_digest);
    let variant_path = key_dir.join(format!("{envelope_digest}.json"));
    let envelope_bytes = serde_json::to_vec(&request.envelope)?;
    let variant_file =
        pending_usage_file(&artifact_root, &variant_path, envelope_bytes.len() as u64)?;
    let commit_file = variant_file.path.clone();
    let _entry_lock =
        acquire_artifact_lock(entry_lock_path(&artifact_root, &owner, &entry_digest)).await?;
    let locked_blob_locks: BTreeSet<String> = required_by_lock.keys().cloned().collect();
    let preserved_blob_ids: BTreeSet<String> =
        required_by_lock.values().flatten().map(|(_, id, _)| id.clone()).collect();
    let _blob_locks = loop {
        let owner_recovery_lock =
            acquire_artifact_lock(owner_lock_path(&artifact_root, &owner)).await?;
        let mut blob_locks = Vec::with_capacity(locked_blob_locks.len());
        let mut acquired_all_blob_locks = true;
        for lock in &locked_blob_locks {
            let Some(blob_lock) =
                try_acquire_artifact_lock(blob_lock_path_for_key(&artifact_root, lock)).await?
            else {
                acquired_all_blob_locks = false;
                break;
            };
            blob_locks.push(blob_lock);
        }
        if acquired_all_blob_locks
            && reconcile_storage_reservations(
                &artifact_root,
                &ArtifactRecoveryLocks {
                    owner: &owner,
                    publication: &publication_lock,
                    active_reservation: Some(&reservation),
                    adoption_commit_file: Some(&commit_file),
                    owner_locked: true,
                    blob_locks: &locked_blob_locks,
                    preserved_blob_ids: &preserved_blob_ids,
                },
            )
            .await?
        {
            break blob_locks;
        }
        drop(blob_locks);
        drop(owner_recovery_lock);
        sleep(ARTIFACT_LOCK_POLL_INTERVAL).await;
    };

    let publication_result: Result<bool> = async {
        let mut new_blobs = Vec::new();
        for blobs in required_by_lock.into_values() {
            for (integrity, id, size) in blobs {
                let path = blobs_dir.join(&id);
                match uploads.remove(integrity) {
                    Some(bytes) => {
                        if !fs::try_exists(&path).await? {
                            new_blobs.push((path, bytes));
                        }
                    }
                    None => match fs::read(&path).await {
                        Ok(bytes) => {
                            if bytes.len() as u64 != size {
                                return Err(RegistryError::Internal {
                                    reason: format!(
                                        "stored shared artifact blob {id} has {} bytes instead of {size}",
                                        bytes.len(),
                                    ),
                                });
                            }
                            verify_blob(integrity, &bytes).map_err(|err| {
                                RegistryError::Internal {
                                    reason: format!(
                                        "stored shared artifact blob failed verification: {err}",
                                    ),
                                }
                            })?;
                        }
                        Err(err) if err.kind() == ErrorKind::NotFound => {
                            return Err(bad_request(format!(
                                "signed manifest references blob {id} without uploading it",
                            )));
                        }
                        Err(err) => return Err(err.into()),
                    },
                }
            }
        }

        if fs::try_exists(&variant_path).await? {
            remove_storage_reservation(&artifact_root, &reservation).await?;
            return Ok(false);
        }
        if count_variants(&key_dir).await? >= MAX_VARIANTS_PER_CANDIDATE {
            return Err(bad_request(format!(
                "artifact key already has the {MAX_VARIANTS_PER_CANDIDATE}-variant limit",
            )));
        }

        let mut pending_files = Vec::with_capacity(new_blobs.len() + 1);
        for (path, bytes) in &new_blobs {
            pending_files.push(pending_usage_file(&artifact_root, path, bytes.len() as u64)?);
        }
        pending_files.push(variant_file);
        reserve_storage_quota(
            &artifact_root,
            StorageQuotaReservation {
                id: &reservation,
                owner: &owner,
                lock: publication_lock.clone(),
                commit_file: Some(commit_file),
                locked_blob_locks: &locked_blob_locks,
                files: pending_files,
            },
        )
        .await?;
        for (path, bytes) in new_blobs {
            write_atomic(&path, &bytes).await?;
        }
        write_atomic(&variant_path, &envelope_bytes).await?;
        if !remove_storage_reservation(&artifact_root, &reservation).await? {
            return Err(RegistryError::Internal {
                reason: "shared artifact storage reservation is missing after publication"
                    .to_string(),
            });
        }
        Ok(true)
    }
    .await;
    match publication_result {
        Ok(published) => Ok(published),
        Err(error) => {
            let preserved_blob_ids = BTreeSet::new();
            if let Err(rollback_error) = reconcile_storage_reservations(
                &artifact_root,
                &ArtifactRecoveryLocks {
                    owner: &owner,
                    publication: &publication_lock,
                    active_reservation: None,
                    adoption_commit_file: None,
                    owner_locked: false,
                    blob_locks: &locked_blob_locks,
                    preserved_blob_ids: &preserved_blob_ids,
                },
            )
            .await
            {
                return Err(RegistryError::Internal {
                    reason: format!(
                        "shared artifact publication failed: {error}; rollback failed: {rollback_error}",
                    ),
                });
            }
            Err(error)
        }
    }
}

async fn reserve_storage_quota(
    artifact_root: &Path,
    reservation: StorageQuotaReservation<'_>,
) -> Result<()> {
    reserve_storage_quota_with_locks_and_limits(
        artifact_root,
        reservation,
        MAX_OWNER_ARTIFACT_BYTES,
        MAX_GLOBAL_ARTIFACT_BYTES,
    )
    .await
}

async fn reserve_storage_quota_with_locks_and_limits(
    artifact_root: &Path,
    reservation: StorageQuotaReservation<'_>,
    owner_limit: u64,
    global_limit: u64,
) -> Result<()> {
    let _usage_lock = acquire_artifact_lock(usage_lock_path(artifact_root)).await?;
    let mut usage = load_artifact_usage(artifact_root).await?;
    let preserved_blob_ids = BTreeSet::new();
    let reconciliation = reconcile_pending_usage(
        artifact_root,
        &mut usage,
        &ArtifactRecoveryLocks {
            owner: reservation.owner,
            publication: &reservation.lock,
            active_reservation: Some(reservation.id),
            adoption_commit_file: reservation.commit_file.as_deref(),
            owner_locked: false,
            blob_locks: reservation.locked_blob_locks,
            preserved_blob_ids: &preserved_blob_ids,
        },
    )
    .await?;
    if reconciliation.usage_changed {
        write_artifact_usage(artifact_root, &usage).await?;
    }
    let added_bytes = reservation.files.iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.size).ok_or_else(storage_quota_error)
    })?;
    let owner_bytes = usage.owner_bytes.get(reservation.owner).copied().unwrap_or(0);
    let next_owner_bytes = owner_bytes.checked_add(added_bytes).ok_or_else(storage_quota_error)?;
    let next_global_bytes =
        usage.global_bytes.checked_add(added_bytes).ok_or_else(storage_quota_error)?;
    if next_owner_bytes > owner_limit {
        return Err(storage_quota_error());
    }
    if next_global_bytes > global_limit {
        return Err(storage_quota_error());
    }
    usage.global_bytes = next_global_bytes;
    usage.owner_bytes.insert(reservation.owner.to_string(), next_owner_bytes);
    let pending = PendingUsage {
        owner: reservation.owner.to_string(),
        lock: reservation.lock,
        commit_file: reservation.commit_file,
        files: reservation.files,
    };
    merge_pending_usage(&mut usage, reservation.id, pending)?;
    write_artifact_usage(artifact_root, &usage).await?;
    Ok(())
}

async fn remove_storage_reservation(artifact_root: &Path, reservation: &str) -> Result<bool> {
    let _usage_lock = acquire_artifact_lock(usage_lock_path(artifact_root)).await?;
    let mut usage = load_artifact_usage(artifact_root).await?;
    if usage.pending.remove(reservation).is_none() {
        return Ok(false);
    }
    write_artifact_usage(artifact_root, &usage).await?;
    Ok(true)
}

async fn reconcile_storage_reservations(
    artifact_root: &Path,
    recovery_locks: &ArtifactRecoveryLocks<'_>,
) -> Result<bool> {
    let _usage_lock = acquire_artifact_lock(usage_lock_path(artifact_root)).await?;
    let mut usage = load_artifact_usage(artifact_root).await?;
    let reconciliation = reconcile_pending_usage(artifact_root, &mut usage, recovery_locks).await?;
    if reconciliation.usage_changed {
        write_artifact_usage(artifact_root, &usage).await?;
    }
    Ok(!reconciliation.blocked_locked_blobs)
}

async fn load_artifact_usage(artifact_root: &Path) -> Result<ArtifactUsage> {
    match fs::read(artifact_usage_path(artifact_root)).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(Into::into),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            scan_artifact_usage(artifact_root).await
        }
        Err(error) => Err(error.into()),
    }
}

async fn scan_artifact_usage(artifact_root: &Path) -> Result<ArtifactUsage> {
    let global_bytes = stored_bytes(artifact_root, Some(".locks")).await?;
    let mut owner_bytes = BTreeMap::new();
    let mut entries = match fs::read_dir(artifact_root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ArtifactUsage::default());
        }
        Err(error) => return Err(error.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name() == ".locks" || !entry.file_type().await?.is_dir() {
            continue;
        }
        let owner = entry.file_name().into_string().map_err(|_| RegistryError::Internal {
            reason: "shared artifact owner directory is not valid UTF-8".to_string(),
        })?;
        owner_bytes.insert(owner, stored_bytes(&entry.path(), None).await?);
    }
    Ok(ArtifactUsage { global_bytes, owner_bytes, pending: BTreeMap::new() })
}

async fn reconcile_pending_usage(
    artifact_root: &Path,
    usage: &mut ArtifactUsage,
    recovery_locks: &ArtifactRecoveryLocks<'_>,
) -> Result<PendingUsageReconciliation> {
    let reservations: Vec<String> = usage.pending.keys().cloned().collect();
    let mut reconciliation = PendingUsageReconciliation::default();
    let mut adopted_files = Vec::new();
    for reservation in reservations {
        if recovery_locks.active_reservation == Some(reservation.as_str()) {
            continue;
        }
        let pending = usage.pending.get(&reservation).ok_or_else(missing_pending_usage)?;
        let pending_blob_locks = pending_blob_lock_keys(pending)?;
        let shares_locked_blobs =
            pending_blob_locks.iter().any(|lock| recovery_locks.blob_locks.contains(lock));
        let Some(_reservation_locks) = try_acquire_pending_usage_locks(
            artifact_root,
            pending,
            pending_blob_locks,
            recovery_locks,
        )
        .await?
        else {
            reconciliation.blocked_locked_blobs |= shares_locked_blobs;
            continue;
        };

        let pending = usage.pending.remove(&reservation).ok_or_else(missing_pending_usage)?;
        let committed = pending_usage_is_committed(artifact_root, &pending).await?;
        adopted_files.extend(
            reconcile_pending_files(artifact_root, usage, pending, committed, recovery_locks)
                .await?,
        );
        reconciliation.usage_changed = true;
    }
    if !adopted_files.is_empty() {
        adopt_recovered_files(usage, recovery_locks, adopted_files)?;
        reconciliation.usage_changed = true;
    }
    Ok(reconciliation)
}

struct PendingUsageLocks {
    _publication: Option<File>,
    _blobs: Vec<File>,
}

async fn try_acquire_pending_usage_locks(
    artifact_root: &Path,
    pending: &PendingUsage,
    pending_blob_locks: BTreeSet<String>,
    recovery_locks: &ArtifactRecoveryLocks<'_>,
) -> Result<Option<PendingUsageLocks>> {
    let caller_holds_lock = pending.owner == recovery_locks.owner
        && (&pending.lock == recovery_locks.publication
            || recovery_locks.owner_locked && matches!(&pending.lock, PendingUsageLock::Owner));
    let publication = if caller_holds_lock {
        None
    } else {
        let Some(lock) = try_acquire_artifact_lock(pending_lock_path(
            artifact_root,
            &pending.owner,
            &pending.lock,
        ))
        .await?
        else {
            return Ok(None);
        };
        Some(lock)
    };

    let mut blobs = Vec::new();
    for lock in pending_blob_locks {
        if recovery_locks.blob_locks.contains(&lock) {
            continue;
        }
        let Some(lock) =
            try_acquire_artifact_lock(blob_lock_path_for_key(artifact_root, &lock)).await?
        else {
            return Ok(None);
        };
        blobs.push(lock);
    }
    Ok(Some(PendingUsageLocks { _publication: publication, _blobs: blobs }))
}

async fn pending_usage_is_committed(artifact_root: &Path, pending: &PendingUsage) -> Result<bool> {
    let Some(commit_path) = pending.commit_file.as_deref() else {
        return Ok(true);
    };
    let Some(commit_file) = pending.files.iter().find(|file| file.path == commit_path) else {
        return Ok(false);
    };
    let commit_path = pending_usage_path(commit_path)?;
    stored_file_has_size(&artifact_root.join(commit_path), commit_file.size).await
}

async fn reconcile_pending_files(
    artifact_root: &Path,
    usage: &mut ArtifactUsage,
    pending: PendingUsage,
    committed: bool,
    recovery_locks: &ArtifactRecoveryLocks<'_>,
) -> Result<Vec<PendingUsageFile>> {
    let mut adopted_files = Vec::new();
    for file in pending.files {
        let relative_path = pending_usage_path(&file.path)?;
        let path = artifact_root.join(relative_path);
        remove_atomic_write_temps(&path).await?;

        if committed {
            if !stored_file_has_size(&path, file.size).await? {
                release_reserved_bytes(usage, &pending.owner, file.size)?;
            }
            continue;
        }

        let preserved = pending.owner == recovery_locks.owner
            && pending_blob_id(&pending.owner, &file)?
                .is_some_and(|blob| recovery_locks.preserved_blob_ids.contains(blob));
        if preserved && stored_file_has_size(&path, file.size).await? {
            adopted_files.push(file);
            continue;
        }

        match fs::remove_file(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        release_reserved_bytes(usage, &pending.owner, file.size)?;
    }
    Ok(adopted_files)
}

async fn stored_file_has_size(path: &Path, expected_size: u64) -> Result<bool> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file() && metadata.len() == expected_size),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn adopt_recovered_files(
    usage: &mut ArtifactUsage,
    recovery_locks: &ArtifactRecoveryLocks<'_>,
    files: Vec<PendingUsageFile>,
) -> Result<()> {
    let reservation = recovery_locks.active_reservation.ok_or_else(|| RegistryError::Internal {
        reason: "shared artifact recovery has no adopting storage reservation".to_string(),
    })?;
    let commit_file =
        recovery_locks.adoption_commit_file.ok_or_else(|| RegistryError::Internal {
            reason: "shared artifact recovery has no adopting commit file".to_string(),
        })?;
    merge_pending_usage(
        usage,
        reservation,
        PendingUsage {
            owner: recovery_locks.owner.to_string(),
            lock: recovery_locks.publication.clone(),
            commit_file: Some(commit_file.to_string()),
            files,
        },
    )
}

fn missing_pending_usage() -> RegistryError {
    RegistryError::Internal {
        reason: "shared artifact pending storage reservation disappeared".to_string(),
    }
}

fn merge_pending_usage(
    usage: &mut ArtifactUsage,
    reservation: &str,
    pending: PendingUsage,
) -> Result<()> {
    if let Some(active) = usage.pending.get_mut(reservation) {
        if active.owner != pending.owner
            || active.lock != pending.lock
            || active.commit_file != pending.commit_file
        {
            return Err(RegistryError::Internal {
                reason: "shared artifact publication has conflicting storage reservations"
                    .to_string(),
            });
        }
        active.files.extend(pending.files);
    } else {
        usage.pending.insert(reservation.to_string(), pending);
    }
    Ok(())
}

fn release_reserved_bytes(usage: &mut ArtifactUsage, owner: &str, bytes: u64) -> Result<()> {
    usage.global_bytes =
        usage.global_bytes.checked_sub(bytes).ok_or_else(|| RegistryError::Internal {
            reason: "shared artifact global usage counter underflow".to_string(),
        })?;
    let owner_bytes = usage.owner_bytes.get_mut(owner).ok_or_else(|| RegistryError::Internal {
        reason: "shared artifact usage state is missing the pending owner".to_string(),
    })?;
    *owner_bytes = owner_bytes.checked_sub(bytes).ok_or_else(|| RegistryError::Internal {
        reason: "shared artifact owner usage counter underflow".to_string(),
    })?;
    Ok(())
}

fn pending_blob_lock_keys(pending: &PendingUsage) -> Result<BTreeSet<String>> {
    let mut lock_keys = BTreeSet::new();
    for file in &pending.files {
        if let Some(blob) = pending_blob_id(&pending.owner, file)? {
            lock_keys.insert(blob_lock_key(&pending.owner, blob));
        }
    }
    Ok(lock_keys)
}

fn pending_blob_id<'a>(owner: &str, file: &'a PendingUsageFile) -> Result<Option<&'a str>> {
    let mut components = pending_usage_path(&file.path)?.components();
    let (Some(Component::Normal(file_owner)), Some(Component::Normal(kind))) =
        (components.next(), components.next())
    else {
        return Ok(None);
    };
    if file_owner != owner || kind != "blobs" {
        return Ok(None);
    }
    let (Some(Component::Normal(blob)), None) = (components.next(), components.next()) else {
        return Ok(None);
    };
    blob.to_str().map(Some).ok_or_else(|| RegistryError::Internal {
        reason: "shared artifact usage state contains an invalid blob path".to_string(),
    })
}

fn pending_usage_path(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || !path.components().all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(RegistryError::Internal {
            reason: "shared artifact usage state contains an invalid pending path".to_string(),
        });
    }
    Ok(path)
}

async fn write_artifact_usage(artifact_root: &Path, usage: &ArtifactUsage) -> Result<()> {
    write_atomic(&artifact_usage_path(artifact_root), &serde_json::to_vec(usage)?).await
}

fn artifact_usage_path(artifact_root: &Path) -> PathBuf {
    artifact_root.join(".locks").join(ARTIFACT_USAGE_FILE)
}

fn usage_lock_path(artifact_root: &Path) -> PathBuf {
    artifact_root.join(".locks").join("usage.lock")
}

fn owner_lock_path(artifact_root: &Path, owner: &str) -> PathBuf {
    artifact_root
        .join(".locks")
        .join("owners")
        .join(format!("{}.lock", digest_segment(owner.as_bytes())))
}

fn entry_lock_path(artifact_root: &Path, owner: &str, entry: &str) -> PathBuf {
    artifact_root
        .join(".locks")
        .join("entries")
        .join(digest_segment(owner.as_bytes()))
        .join(format!("{}.lock", digest_segment(entry.as_bytes())))
}

fn blob_lock_key(owner: &str, blob: &str) -> String {
    let mut bytes = Vec::with_capacity(owner.len() + blob.len() + 1);
    bytes.extend_from_slice(owner.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(blob.as_bytes());
    let digest = Sha256::digest(bytes);
    format!("{:02x}", usize::from(digest[0]) % BLOB_LOCK_STRIPES)
}

fn blob_lock_path_for_key(artifact_root: &Path, lock: &str) -> PathBuf {
    artifact_root.join(".locks").join("blobs").join(format!("{lock}.lock"))
}

fn pending_lock_path(
    artifact_root: &Path,
    owner: &str,
    pending_lock: &PendingUsageLock,
) -> PathBuf {
    match pending_lock {
        PendingUsageLock::Entry { entry } => entry_lock_path(artifact_root, owner, entry),
        PendingUsageLock::Owner => owner_lock_path(artifact_root, owner),
        PendingUsageLock::Global => artifact_root.join(".locks").join("publication.lock"),
    }
}

fn reservation_id(owner: &str, entry: &str, envelope: &str) -> String {
    let counter = ARTIFACT_RESERVATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let mut bytes = Vec::with_capacity(owner.len() + entry.len() + envelope.len() + 34);
    bytes.extend_from_slice(owner.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(entry.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(envelope.as_bytes());
    bytes.extend_from_slice(&std::process::id().to_ne_bytes());
    bytes.extend_from_slice(&counter.to_ne_bytes());
    bytes.extend_from_slice(&timestamp.to_ne_bytes());
    digest_segment(&bytes)
}

fn deserialize_pending_usage<'de, Deserializer>(
    deserializer: Deserializer,
) -> std::result::Result<BTreeMap<String, PendingUsage>, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum PendingUsageState {
        Current(BTreeMap<String, PendingUsage>),
        OwnerScoped(BTreeMap<String, Vec<PendingUsageFile>>),
        Global(Option<LegacyPendingUsage>),
    }

    Ok(match <PendingUsageState as serde::Deserialize>::deserialize(deserializer)? {
        PendingUsageState::Current(pending) => pending,
        PendingUsageState::OwnerScoped(pending) => pending
            .into_iter()
            .map(|(owner, files)| {
                let reservation = format!("legacy-owner-{}", digest_segment(owner.as_bytes()));
                (
                    reservation,
                    PendingUsage { owner, lock: PendingUsageLock::Owner, commit_file: None, files },
                )
            })
            .collect(),
        PendingUsageState::Global(Some(pending)) => BTreeMap::from([(
            "legacy-global".to_string(),
            PendingUsage {
                owner: pending.owner,
                lock: PendingUsageLock::Global,
                commit_file: None,
                files: pending.files,
            },
        )]),
        PendingUsageState::Global(None) => BTreeMap::new(),
    })
}

fn pending_usage_file(artifact_root: &Path, path: &Path, size: u64) -> Result<PendingUsageFile> {
    let relative_path = path.strip_prefix(artifact_root).map_err(|_| RegistryError::Internal {
        reason: "shared artifact usage path escaped the artifact root".to_string(),
    })?;
    let path = relative_path.to_str().ok_or_else(|| RegistryError::Internal {
        reason: "shared artifact usage path is not valid UTF-8".to_string(),
    })?;
    Ok(PendingUsageFile { path: path.to_string(), size })
}

fn owner_usage_key(owner_dir: &Path) -> Result<String> {
    owner_dir.file_name().and_then(|name| name.to_str()).map(ToOwned::to_owned).ok_or_else(|| {
        RegistryError::Internal {
            reason: "shared artifact owner directory has no valid name".to_string(),
        }
    })
}

async fn stored_bytes(root: &Path, ignored_root_entry: Option<&str>) -> Result<u64> {
    let mut directories = vec![root.to_path_buf()];
    let mut total = 0_u64;
    while let Some(directory) = directories.pop() {
        let mut entries = match fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            if directory == root
                && ignored_root_entry.is_some_and(|ignored| entry.file_name() == ignored)
            {
                continue;
            }
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                total = total
                    .checked_add(entry.metadata().await?.len())
                    .ok_or_else(storage_quota_error)?;
            }
        }
    }
    Ok(total)
}

fn storage_quota_error() -> RegistryError {
    bad_request(format!(
        "shared artifact storage quota exceeded ({MAX_OWNER_ARTIFACT_BYTES} bytes per owner, {MAX_GLOBAL_ARTIFACT_BYTES} bytes globally)",
    ))
}

pub(crate) async fn resolve(
    cache_storage: &Path,
    username: &str,
    body: &[u8],
) -> Result<ResolveArtifactsResponse> {
    let request: ResolveArtifactsRequest = serde_json::from_slice(body)
        .map_err(|err| bad_request(format!("invalid shared artifact lookup: {err}")))?;
    if request.candidates.len() > MAX_CANDIDATES {
        return Err(bad_request(format!(
            "lookup contains {} candidates; limit is {MAX_CANDIDATES}",
            request.candidates.len(),
        )));
    }
    let mut seen = HashSet::with_capacity(request.candidates.len());
    let mut artifacts = Vec::new();
    let mut budget = ResolveBudget {
        used_bytes: serde_json::to_vec(&ResolveArtifactsResponse { artifacts: Vec::new() })?.len(),
    };
    for candidate in request.candidates {
        candidate.validate().map_err(|err| protocol_error(&err))?;
        if !seen.insert(candidate.key.clone()) {
            return Err(bad_request("lookup contains a duplicate candidate".to_string()));
        }
        let Some(resolved) =
            resolve_candidate(cache_storage, username, &candidate, &mut budget).await?
        else {
            continue;
        };
        budget.add_response(&resolved, !artifacts.is_empty())?;
        artifacts.push(resolved);
    }
    Ok(ResolveArtifactsResponse { artifacts })
}

pub(crate) async fn read_blob(
    cache_storage: &Path,
    username: &str,
    body: &[u8],
) -> Result<Option<(fs::File, u64)>> {
    let request: ArtifactBlobRequest = serde_json::from_slice(body)
        .map_err(|err| bad_request(format!("invalid artifact blob request: {err}")))?;
    request.validate().map_err(|err| protocol_error(&err))?;
    let owner_dir = match owner_dir(cache_storage, username, &request.owner) {
        Ok(path) => path,
        Err(RegistryError::Forbidden { .. }) => return Ok(None),
        Err(err) => return Err(err),
    };
    let id = blob_id(&request.integrity).map_err(|err| protocol_error(&err))?;
    let mut file = match fs::File::open(owner_dir.join("blobs").join(&id)).await {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let size = file.metadata().await?.len();
    if size > MAX_FILE_SIZE {
        return Err(RegistryError::Internal {
            reason: format!(
                "stored shared artifact blob has {size} bytes; limit is {MAX_FILE_SIZE}",
            ),
        });
    }
    let mut digest = Sha512::new();
    let mut buffer = vec![0_u8; ARTIFACT_BLOB_READ_CHUNK];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    if hex(&digest.finalize()) != id {
        return Err(RegistryError::Internal {
            reason: "stored shared artifact blob failed verification: downloaded bytes do not match the declared digest"
                .to_string(),
        });
    }
    file.seek(SeekFrom::Start(0)).await?;
    Ok(Some((file, size)))
}

async fn resolve_candidate(
    cache_storage: &Path,
    username: &str,
    candidate: &ArtifactCandidate,
    budget: &mut ResolveBudget,
) -> Result<Option<ResolvedArtifact>> {
    let owner_dir = match owner_dir(cache_storage, username, &candidate.owner) {
        Ok(path) => path,
        Err(RegistryError::Forbidden { .. }) => return Ok(None),
        Err(err) => return Err(err),
    };
    let key_dir = owner_dir.join("entries").join(entry_digest(
        &candidate.key,
        &candidate.package,
        &candidate.source_integrity,
    ));
    let mut entries = match fs::read_dir(key_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() && is_variant_file(&entry.file_name()) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    let mut variants = Vec::new();
    for path in paths.into_iter().take(MAX_VARIANTS_PER_CANDIDATE) {
        budget.add_scan(fs::metadata(&path).await?.len())?;
        let bytes = fs::read(&path).await?;
        let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
            continue;
        };
        let Ok((payload, _)) = envelope.decode_payload() else {
            continue;
        };
        if artifact_matches_candidate(&payload, candidate) {
            variants.push(ArtifactVariant { envelope });
        }
    }
    Ok((!variants.is_empty()).then(|| ResolvedArtifact { key: candidate.key.clone(), variants }))
}

struct ResolveBudget {
    used_bytes: usize,
}

impl ResolveBudget {
    fn add_scan(&mut self, bytes: u64) -> Result<()> {
        self.add(usize::try_from(bytes).unwrap_or(usize::MAX))
    }

    fn add_response(&mut self, artifact: &ResolvedArtifact, needs_comma: bool) -> Result<()> {
        let serialized_size = serde_json::to_vec(artifact)?.len() + usize::from(needs_comma);
        self.add(serialized_size)
    }

    fn add(&mut self, bytes: usize) -> Result<()> {
        self.used_bytes = self.used_bytes.checked_add(bytes).ok_or_else(resolve_limit_error)?;
        if self.used_bytes > MAX_RESOLVE_RESPONSE_SIZE {
            return Err(resolve_limit_error());
        }
        Ok(())
    }
}

fn resolve_limit_error() -> RegistryError {
    bad_request(format!(
        "shared artifact lookup exceeds the {MAX_RESOLVE_RESPONSE_SIZE}-byte budget",
    ))
}

fn owner_dir(cache_storage: &Path, username: &str, owner: &OwnerScope) -> Result<PathBuf> {
    match owner {
        OwnerScope::Organization { name } if name == username => Ok(cache_storage
            .join(ARTIFACT_CACHE_DIR)
            .join(digest_segment(owner.namespace().as_bytes()))),
        OwnerScope::Organization { .. } | OwnerScope::Publisher { .. } => {
            Err(RegistryError::Forbidden {
                user: username.to_string(),
                action: "access shared artifacts owned by",
                resource: owner.namespace(),
            })
        }
    }
}

async fn acquire_artifact_lock(path: PathBuf) -> Result<File> {
    let file = tokio::task::spawn_blocking(move || open_lock_file(&path)).await??;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => sleep(ARTIFACT_LOCK_POLL_INTERVAL).await,
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

async fn try_acquire_artifact_lock(path: PathBuf) -> Result<Option<File>> {
    let file = tokio::task::spawn_blocking(move || open_lock_file(&path)).await??;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

async fn count_variants(key_dir: &Path) -> Result<usize> {
    let mut entries = match fs::read_dir(key_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };
    let mut count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() && is_variant_file(&entry.file_name()) {
            count += 1;
        }
    }
    Ok(count)
}

fn is_variant_file(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        let bytes = name.as_bytes();
        bytes.len() == 69
            && bytes[64..] == *b".json"
            && bytes[..64].iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    })
}

fn digest_segment(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
        output
    })
}

fn entry_digest(key: &str, package: &PackageIdentity, source_integrity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-shared-artifact-entry-v0\0");
    hasher.update(key.as_bytes());
    hasher.update([0]);
    hasher.update(package.name.as_bytes());
    hasher.update([0]);
    hasher.update(package.version.as_bytes());
    hasher.update([0]);
    hasher.update(source_integrity.as_bytes());
    hex(&hasher.finalize())
}

fn artifact_matches_candidate(payload: &ArtifactPayload, candidate: &ArtifactCandidate) -> bool {
    let ArtifactCandidate { key: input_key, package, source_integrity, owner } = candidate;
    payload.input_key == *input_key
        && payload.package == *package
        && payload.source_integrity == *source_integrity
        && payload.owner == *owner
}

fn protocol_error(error: &ArtifactProtocolError) -> RegistryError {
    bad_request(error.to_string())
}

fn bad_request(reason: String) -> RegistryError {
    RegistryError::BadRequest { reason }
}

#[cfg(test)]
mod tests;
