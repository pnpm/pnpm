use std::{
    collections::{BTreeMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};

use pnpm_fs::DirLock;
use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactProtocolError,
    ArtifactVariant, MAX_CANDIDATES, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE,
    OwnerScope, PackageIdentity, PublishArtifactRequest, ResolveArtifactsRequest,
    ResolveArtifactsResponse, ResolvedArtifact, SignedArtifactEnvelope, blob_id, verify_blob,
};
use sha2::{Digest as _, Sha256};
use tokio::fs;

use crate::{
    error::{RegistryError, Result},
    storage::write_atomic,
};

const ARTIFACT_CACHE_DIR: &str = "shared-artifacts/v0";
const ARTIFACT_LOCK_WAIT: Duration = Duration::from_secs(30);
const ARTIFACT_LOCK_ABANDONED_AFTER: Duration = Duration::from_hours(1);
const MAX_OWNER_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_GLOBAL_ARTIFACT_BYTES: u64 = 10 * MAX_OWNER_ARTIFACT_BYTES;

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
    let entry_digest = entry_digest(&request.key, &payload.package, &payload.source_integrity);
    let key_dir = owner_dir.join("entries").join(&entry_digest);
    let _publication_lock =
        acquire_publication_lock(artifact_root.join(".locks").join("publication.lock")).await?;
    let envelope_digest = request.envelope.digest().map_err(|err| protocol_error(&err))?;
    let variant_path = key_dir.join(format!("{envelope_digest}.json"));
    let already_present = fs::try_exists(&variant_path).await?;
    if !already_present && count_variants(&key_dir).await? >= MAX_VARIANTS_PER_CANDIDATE {
        return Err(bad_request(format!(
            "artifact key already has the {MAX_VARIANTS_PER_CANDIDATE}-variant limit",
        )));
    }

    let required: BTreeMap<&str, u64> =
        payload.manifest.added.iter().map(|file| (file.integrity.as_str(), file.size)).collect();

    let envelope_bytes = serde_json::to_vec(&request.envelope)?;
    let blobs_dir = owner_dir.join("blobs");
    let mut new_blobs = Vec::new();
    let mut added_bytes = if already_present { 0 } else { envelope_bytes.len() as u64 };
    for (integrity, size) in required {
        let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
        let path = blobs_dir.join(&id);
        let (bytes, is_new) = match uploads.remove(integrity) {
            Some(bytes) => {
                let is_new = !fs::try_exists(&path).await?;
                (bytes, is_new)
            }
            None => match fs::read(&path).await {
                Ok(bytes) => (bytes, false),
                Err(err) if err.kind() == ErrorKind::NotFound => {
                    return Err(bad_request(format!(
                        "signed manifest references blob {id} without uploading it",
                    )));
                }
                Err(err) => return Err(err.into()),
            },
        };
        if bytes.len() as u64 != size {
            return Err(bad_request(format!(
                "blob {id} has {} bytes but the signed manifest declares {}",
                bytes.len(),
                size,
            )));
        }
        verify_blob(integrity, &bytes).map_err(|err| protocol_error(&err))?;
        if is_new {
            added_bytes =
                added_bytes.checked_add(bytes.len() as u64).ok_or_else(storage_quota_error)?;
            new_blobs.push((path, bytes));
        }
    }

    enforce_storage_quota(&artifact_root, &owner_dir, added_bytes).await?;
    for (path, bytes) in new_blobs {
        write_atomic(&path, &bytes).await?;
    }
    if !already_present {
        write_atomic(&variant_path, &envelope_bytes).await?;
    }
    Ok(!already_present)
}

async fn enforce_storage_quota(
    artifact_root: &Path,
    owner_dir: &Path,
    added_bytes: u64,
) -> Result<()> {
    enforce_storage_quota_with_limits(
        artifact_root,
        owner_dir,
        added_bytes,
        MAX_OWNER_ARTIFACT_BYTES,
        MAX_GLOBAL_ARTIFACT_BYTES,
    )
    .await
}

async fn enforce_storage_quota_with_limits(
    artifact_root: &Path,
    owner_dir: &Path,
    added_bytes: u64,
    owner_limit: u64,
    global_limit: u64,
) -> Result<()> {
    let owner_bytes = stored_bytes(owner_dir, None);
    let global_bytes = stored_bytes(artifact_root, Some(".locks"));
    let (owner_bytes, global_bytes) = tokio::try_join!(owner_bytes, global_bytes)?;
    if owner_bytes.checked_add(added_bytes).is_none_or(|total| total > owner_limit) {
        return Err(storage_quota_error());
    }
    if global_bytes.checked_add(added_bytes).is_none_or(|total| total > global_limit) {
        return Err(storage_quota_error());
    }
    Ok(())
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
) -> Result<Option<Vec<u8>>> {
    let request: ArtifactBlobRequest = serde_json::from_slice(body)
        .map_err(|err| bad_request(format!("invalid artifact blob request: {err}")))?;
    request.validate().map_err(|err| protocol_error(&err))?;
    let owner_dir = match owner_dir(cache_storage, username, &request.owner) {
        Ok(path) => path,
        Err(RegistryError::Forbidden { .. }) => return Ok(None),
        Err(err) => return Err(err),
    };
    let id = blob_id(&request.integrity).map_err(|err| protocol_error(&err))?;
    match fs::read(owner_dir.join("blobs").join(id)).await {
        Ok(bytes) => {
            verify_blob(&request.integrity, &bytes).map_err(|err| RegistryError::Internal {
                reason: format!("stored shared artifact blob failed verification: {err}"),
            })?;
            Ok(Some(bytes))
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
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

async fn acquire_publication_lock(path: PathBuf) -> Result<DirLock> {
    tokio::task::spawn_blocking(move || {
        DirLock::acquire(path, ARTIFACT_LOCK_WAIT, ARTIFACT_LOCK_ABANDONED_AFTER)
    })
    .await??
    .ok_or_else(|| RegistryError::Internal {
        reason: "timed out acquiring shared artifact publication lock".to_string(),
    })
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
