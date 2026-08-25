use std::{
    collections::{BTreeMap, HashSet},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactProtocolError,
    ArtifactVariant, MAX_CANDIDATES, MAX_VARIANTS_PER_CANDIDATE, OwnerScope,
    PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact,
    SignedArtifactEnvelope, blob_id, verify_blob,
};
use sha2::{Digest as _, Sha256};
use tokio::fs;

use crate::{
    error::{RegistryError, Result},
    storage::write_atomic,
};

const ARTIFACT_CACHE_DIR: &str = "shared-artifacts/v0";

pub(crate) async fn publish(cache_storage: &Path, username: &str, body: &[u8]) -> Result<bool> {
    let request: PublishArtifactRequest = serde_json::from_slice(body)
        .map_err(|err| bad_request(format!("invalid shared artifact request: {err}")))?;
    let (payload, _) = request.envelope.decode_payload().map_err(|err| protocol_error(&err))?;
    let owner_dir = owner_dir(cache_storage, username, &payload.owner)?;
    if payload.input_key != request.key {
        return Err(bad_request("signed input key does not match the request key".to_string()));
    }
    let key_dir = owner_dir.join("entries").join(digest_segment(request.key.as_bytes()));
    let envelope_digest = request.envelope.digest().map_err(|err| protocol_error(&err))?;
    let variant_path = key_dir.join(format!("{envelope_digest}.json"));
    let already_present = fs::try_exists(&variant_path).await?;
    if !already_present && count_variants(&key_dir).await? >= MAX_VARIANTS_PER_CANDIDATE {
        return Err(bad_request(format!(
            "artifact key already has the {MAX_VARIANTS_PER_CANDIDATE}-variant limit",
        )));
    }

    let mut uploads = BTreeMap::new();
    for blob in request.blobs {
        if uploads.insert(blob.integrity.clone(), blob.data).is_some() {
            return Err(bad_request(format!("duplicate blob upload for {:?}", blob.integrity)));
        }
    }
    let required: HashSet<&str> =
        payload.manifest.added.iter().map(|file| file.integrity.as_str()).collect();
    if uploads.keys().any(|integrity| !required.contains(integrity.as_str())) {
        return Err(bad_request(
            "request includes a blob that is not referenced by the signed manifest".to_string(),
        ));
    }

    let blobs_dir = owner_dir.join("blobs");
    for file in &payload.manifest.added {
        let id = blob_id(&file.integrity).map_err(|err| protocol_error(&err))?;
        let path = blobs_dir.join(&id);
        let bytes = match uploads.remove(&file.integrity) {
            Some(encoded) => BASE64
                .decode(encoded)
                .map_err(|_| bad_request(format!("blob {id} is not valid base64")))?,
            None => match fs::read(&path).await {
                Ok(bytes) => bytes,
                Err(err) if err.kind() == ErrorKind::NotFound => {
                    return Err(bad_request(format!(
                        "signed manifest references blob {id} without uploading it",
                    )));
                }
                Err(err) => return Err(err.into()),
            },
        };
        if bytes.len() as u64 != file.size {
            return Err(bad_request(format!(
                "blob {id} has {} bytes but the signed manifest declares {}",
                bytes.len(),
                file.size,
            )));
        }
        verify_blob(&file.integrity, &bytes).map_err(|err| protocol_error(&err))?;
        write_atomic(&path, &bytes).await?;
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
    for candidate in request.candidates {
        candidate.validate().map_err(|err| protocol_error(&err))?;
        if !seen.insert(candidate.key.clone()) {
            return Err(bad_request("lookup contains a duplicate candidate".to_string()));
        }
        let Some(resolved) = resolve_candidate(cache_storage, username, &candidate).await? else {
            continue;
        };
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
        let bytes = fs::read(&path).await?;
        let envelope: SignedArtifactEnvelope =
            serde_json::from_slice(&bytes).map_err(|err| RegistryError::Internal {
                reason: format!("stored shared artifact envelope {path:?} is invalid: {err}"),
            })?;
        let (payload, _) = envelope.decode_payload().map_err(|err| RegistryError::Internal {
            reason: format!("stored shared artifact envelope {path:?} is invalid: {err}"),
        })?;
        if artifact_matches_candidate(&payload, candidate) {
            variants.push(ArtifactVariant { envelope });
        }
    }
    Ok((!variants.is_empty()).then(|| ResolvedArtifact { key: candidate.key.clone(), variants }))
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
