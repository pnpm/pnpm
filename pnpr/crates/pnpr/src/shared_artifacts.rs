use std::{
    collections::{BTreeMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactProtocolError,
    ArtifactVariant, MAX_CANDIDATES, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE,
    OwnerScope, PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse,
    ResolvedArtifact, SignedArtifactEnvelope, blob_id, verify_blob,
};
use sha2::{Digest as _, Sha256};
use tokio::fs;

use crate::{
    error::{RegistryError, Result},
    storage::write_atomic,
};

const ARTIFACT_CACHE_DIR: &str = "shared-artifacts/v0";

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
    let owner_dir = owner_dir(cache_storage, username, &payload.owner)?;
    let key_dir = owner_dir.join("entries").join(digest_segment(request.key.as_bytes()));
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

    let blobs_dir = owner_dir.join("blobs");
    for (integrity, size) in required {
        let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
        let path = blobs_dir.join(&id);
        let (bytes, uploaded) = match uploads.remove(integrity) {
            Some(bytes) => (bytes, true),
            None => (
                match fs::read(&path).await {
                    Ok(bytes) => bytes,
                    Err(err) if err.kind() == ErrorKind::NotFound => {
                        return Err(bad_request(format!(
                            "signed manifest references blob {id} without uploading it",
                        )));
                    }
                    Err(err) => return Err(err.into()),
                },
                false,
            ),
        };
        if bytes.len() as u64 != size {
            return Err(bad_request(format!(
                "blob {id} has {} bytes but the signed manifest declares {}",
                bytes.len(),
                size,
            )));
        }
        verify_blob(integrity, &bytes).map_err(|err| protocol_error(&err))?;
        if uploaded && !fs::try_exists(&path).await? {
            write_atomic(&path, &bytes).await?;
        }
    }

    if !already_present {
        let envelope_bytes = serde_json::to_vec(&request.envelope)?;
        write_atomic(&variant_path, &envelope_bytes).await?;
    }
    Ok(!already_present)
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
        scanned_bytes: 0,
        response_bytes: serde_json::to_vec(&ResolveArtifactsResponse { artifacts: Vec::new() })?
            .len(),
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
    let key_dir = owner_dir.join("entries").join(digest_segment(candidate.key.as_bytes()));
    let mut entries = match fs::read_dir(key_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
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
    scanned_bytes: usize,
    response_bytes: usize,
}

impl ResolveBudget {
    fn add_scan(&mut self, bytes: u64) -> Result<()> {
        self.scanned_bytes = self
            .scanned_bytes
            .checked_add(usize::try_from(bytes).unwrap_or(usize::MAX))
            .ok_or_else(resolve_limit_error)?;
        if self.scanned_bytes > MAX_RESOLVE_RESPONSE_SIZE {
            return Err(resolve_limit_error());
        }
        Ok(())
    }

    fn add_response(&mut self, artifact: &ResolvedArtifact, needs_comma: bool) -> Result<()> {
        let serialized_size = serde_json::to_vec(artifact)?.len() + usize::from(needs_comma);
        self.response_bytes =
            self.response_bytes.checked_add(serialized_size).ok_or_else(resolve_limit_error)?;
        if self.response_bytes > MAX_RESOLVE_RESPONSE_SIZE {
            return Err(resolve_limit_error());
        }
        Ok(())
    }
}

fn resolve_limit_error() -> RegistryError {
    bad_request(format!(
        "shared artifact lookup exceeds the {MAX_RESOLVE_RESPONSE_SIZE}-byte response budget",
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

async fn count_variants(key_dir: &Path) -> Result<usize> {
    let mut entries = match fs::read_dir(key_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };
    let mut count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            count += 1;
        }
    }
    Ok(count)
}

fn digest_segment(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().fold(String::with_capacity(64), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
        output
    })
}

fn artifact_matches_candidate(payload: &ArtifactPayload, candidate: &ArtifactCandidate) -> bool {
    let ArtifactCandidate { key: input_key, source_integrity, owner } = candidate;
    payload.input_key == *input_key
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
