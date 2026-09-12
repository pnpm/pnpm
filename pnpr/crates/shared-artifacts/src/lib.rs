pub use compiler_cache::{CompilerCacheKey, MAX_COMPILER_CACHE_ENTRY_SIZE};

mod publication_quota;
use publication_quota::{
    PublicationQuota, expire_stranded_publications, finish_outcome, publication_charge,
    quota_write_retry_delay, register_publication, registered_now,
};

mod artifact_identity;
use artifact_identity::{
    artifact_matches_candidate, artifact_operation_id, compatibility_slot, digest_segment,
    entry_digest, entry_owner, is_blob_path, is_variant_file, object_name, owner_key,
    scope_marker_path, scope_name, scopes_prefix,
};

mod object_storage;

mod reclamation;

mod quota;

mod scopes;

mod publication;

mod compiler_cache;

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use futures_util::{StreamExt as _, stream::BoxStream};
use object_store::{
    ObjectMeta, ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion,
    local::LocalFileSystem, path::Path as ObjectPath,
};
use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactProtocolError,
    ArtifactSubject, ArtifactVariant, CompatibilityConstraints, CompatibilityScopes,
    MAX_CANDIDATES, MAX_FILE_SIZE, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE,
    OwnerScope, PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse,
    ResolvedArtifact, SignedArtifactEnvelope, blob_id, compatibility_scopes, verify_blob,
};
use pnpr_config::{HostedStoreConfig, build_s3_store, normalize_key_prefix};
use pnpr_error::{RegistryError, Result};
use sha2::Sha256;
use tokio::time::{interval, sleep};

const ARTIFACT_CACHE_DIR: &str = "shared-artifacts/v0";
const ARTIFACT_OBJECT_PREFIX: &str = ".pnpr-artifacts/v0";
const ARTIFACT_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const ARTIFACT_USAGE_FILE: &str = ".locks/usage.json";
const ARTIFACT_QUOTA_OBJECT: &str = "quota.json";
const MAX_OWNER_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_GLOBAL_ARTIFACT_BYTES: u64 = 10 * MAX_OWNER_ARTIFACT_BYTES;
const MAX_ACTIVE_PUBLICATIONS: usize = 1024;
const PUBLICATION_FINISH_RETRIES: usize = 8;
/// How long a publication may hold its registration before reclamation treats
/// it as gone.
///
/// A publication that cannot unregister itself — every retry of the write
/// failing — would otherwise hold the gate forever, and reclamation is what
/// gives back the scopes a failed publication claimed. The bound is far longer
/// than a publication that is merely slow, since expiring a live one lets a
/// collector run beside it.
const ACTIVE_PUBLICATION_EXPIRY: Duration = Duration::from_hours(1);
/// How often a publication says it is still working.
///
/// Well inside the expiry, so several renewals have to fail before a
/// publication that is running is mistaken for one that stopped.
const PUBLICATION_RENEWAL_INTERVAL: Duration = Duration::from_mins(5);
const QUOTA_WRITE_RETRIES: usize = 32;
const RECLAMATION_WAIT_RETRIES: usize = 600;
/// A scope marker holds the envelope digest of the artifact that claimed it,
/// which is a hex digest.
const MAX_SCOPE_MARKER_BYTES: u64 = 128;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct ArtifactUsage {
    global_bytes: u64,
    owner_bytes: BTreeMap<String, u64>,
    #[serde(default)]
    active_publications: BTreeSet<String>,
    /// When each publication in flight registered, so one that never
    /// unregistered can be told from one still working.
    ///
    /// Beside the set rather than replacing it: a replica running an older
    /// build shares this document, reads the set it knows, and ignores this.
    /// One that writes drops these, and the next expiry pass stamps them again.
    #[serde(default)]
    active_publication_times: BTreeMap<String, u64>,
    #[serde(default)]
    reclamation_needed: bool,
    #[serde(default)]
    reclamation: Option<String>,
}

#[derive(Debug)]
enum QuotaCoordination {
    Local { lock_path: PathBuf },
    Conditional,
}

#[derive(Clone, Copy)]
enum QuotaChange {
    Reserve,
    Release,
}

pub struct ArtifactBlob {
    pub size: u64,
    pub stream: BoxStream<'static, object_store::Result<Bytes>>,
}

/// What the slot a publication is claiming already holds.
/// The reserved scope key for an artifact that reaches every machine. No tag
/// yields it: every tag key carries an architecture, which this does not.
const UNIVERSAL_SCOPE: &str = "universal";

/// Marks an entry whose artifacts have all been given the scopes they reach. No
/// tag yields it, for the same reason no tag yields [`UNIVERSAL_SCOPE`].
const BACKFILLED_SCOPE: &str = "backfilled";

/// What one pass over a store found: the blobs its artifacts reference, the
/// artifacts themselves, and whether every variant could be read.
struct StoredArtifacts {
    referenced_blobs: HashSet<String>,
    digests: HashSet<String>,
    every_variant_read: bool,
}

impl Default for StoredArtifacts {
    fn default() -> Self {
        Self { referenced_blobs: HashSet::new(), digests: HashSet::new(), every_variant_read: true }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ScopeMarker {
    /// Nobody holds the scope.
    Gone,
    /// The artifact being published holds it.
    Ours,
    /// Another artifact holds it.
    Another,
}

enum SlotClaim {
    /// Every scope this artifact reaches is now claimed for it.
    Free,
    /// This exact envelope, so publishing it again is a retry.
    Held,
    HeldByAnother,
}

struct PreparedPublication {
    /// When this publication began, which is what its registration is measured
    /// against. Taken before the registration rather than after the reads that
    /// follow it, so a slow read cannot let the registration expire while this
    /// still counts itself young.
    started: std::time::Instant,
    entry: String,
    /// Identifies the artifact itself, where the slot identifies only what it is
    /// built for, so a scope marker names which artifact holds it.
    envelope_digest: String,
    payload: ArtifactPayload,
    uploads: BTreeMap<String, Vec<u8>>,
    owner: String,
    envelope_bytes: Vec<u8>,
    variant_path: String,
}

/// Shared build-artifact storage. Local deployments use the
/// `cache/shared-artifacts/v0` layout. Object-store deployments use the same
/// configured bucket as hosted packages under a reserved namespace, allowing
/// every replica to observe the same immutable blobs and envelopes. The quota
/// document also acts as a distributed reclamation gate: publications register
/// before reading objects, and a collector can start only after that set drains.
#[derive(Debug)]
pub struct SharedArtifactStore {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    quota: QuotaCoordination,
    owner_limit: u64,
    global_limit: u64,
}

impl SharedArtifactStore {
    pub fn new(hosted: &HostedStoreConfig, cache_storage: &Path) -> Result<Self> {
        match hosted {
            HostedStoreConfig::Fs => {
                let root = cache_storage.join(ARTIFACT_CACHE_DIR);
                std::fs::create_dir_all(&root)?;
                let store: Arc<dyn ObjectStore> =
                    Arc::new(LocalFileSystem::new_with_prefix(&root)?);
                Ok(Self {
                    store,
                    prefix: String::new(),
                    quota: QuotaCoordination::Local {
                        lock_path: root.join(".locks").join("usage.lock"),
                    },
                    owner_limit: MAX_OWNER_ARTIFACT_BYTES,
                    global_limit: MAX_GLOBAL_ARTIFACT_BYTES,
                })
            }
            HostedStoreConfig::S3(settings) => {
                Ok(Self::object_store(build_s3_store(settings)?, &settings.normalized_prefix()))
            }
            HostedStoreConfig::ObjectStore { store, prefix } => {
                Ok(Self::object_store(Arc::clone(store), &normalize_key_prefix(Some(prefix))))
            }
        }
    }

    pub async fn resolve(&self, username: &str, body: &[u8]) -> Result<ResolveArtifactsResponse> {
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
            used_bytes: serde_json::to_vec(&ResolveArtifactsResponse { artifacts: Vec::new() })?
                .len(),
        };
        for candidate in request.candidates {
            candidate.validate().map_err(|err| protocol_error(&err))?;
            if !seen.insert(candidate.key.clone()) {
                return Err(bad_request("lookup contains a duplicate candidate".to_string()));
            }
            let Some(resolved) = self.resolve_candidate(username, &candidate, &mut budget).await?
            else {
                continue;
            };
            budget.add_response(&resolved, !artifacts.is_empty())?;
            artifacts.push(resolved);
        }
        Ok(ResolveArtifactsResponse { artifacts })
    }

    pub async fn read_blob(&self, username: &str, body: &[u8]) -> Result<Option<ArtifactBlob>> {
        let request: ArtifactBlobRequest = serde_json::from_slice(body)
            .map_err(|err| bad_request(format!("invalid artifact blob request: {err}")))?;
        request.validate().map_err(|err| protocol_error(&err))?;
        let owner = match owner_key(username, &request.owner) {
            Ok(owner) => owner,
            Err(RegistryError::Forbidden { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let id = blob_id(&request.integrity).map_err(|err| protocol_error(&err))?;
        let path = self.object_path(&format!("{owner}/blobs/{id}"));
        let result = match self.store.get(&path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if result.meta.size > MAX_FILE_SIZE {
            return Err(stored_object_too_large(result.meta.size, MAX_FILE_SIZE));
        }
        Ok(Some(ArtifactBlob { size: result.meta.size, stream: result.into_stream() }))
    }

    async fn resolve_candidate(
        &self,
        username: &str,
        candidate: &ArtifactCandidate,
        budget: &mut ResolveBudget,
    ) -> Result<Option<ResolvedArtifact>> {
        let owner = match owner_key(username, &candidate.owner) {
            Ok(owner) => owner,
            Err(RegistryError::Forbidden { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let entry = entry_digest(&candidate.key, &candidate.subject);
        let prefix = format!("{owner}/entries/{entry}/");
        let prefix = self.object_path(&prefix);
        let mut listing = self.store.list(Some(&prefix));
        let mut variants = Vec::new();
        let mut scanned_variants = 0;
        while scanned_variants < MAX_VARIANTS_PER_CANDIDATE {
            let Some(entry) = listing.next().await else { break };
            let entry = entry?;
            if !is_variant_file(object_name(&entry.location)) {
                continue;
            }
            scanned_variants += 1;
            budget.add_scan(entry.size)?;
            let Some(bytes) = self.read_object_path(&entry.location).await? else {
                continue;
            };
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
        Ok((!variants.is_empty())
            .then(|| ResolvedArtifact { key: candidate.key.clone(), variants }))
    }

    #[cfg(test)]
    fn with_limits(mut self, owner_limit: u64, global_limit: u64) -> Self {
        self.owner_limit = owner_limit;
        self.global_limit = global_limit;
        self
    }
}

pub fn parse_publish(body: &[u8]) -> Result<PublishArtifactRequest> {
    serde_json::from_slice(body)
        .map_err(|err| bad_request(format!("invalid shared artifact request: {err}")))
}

fn prepare_publication(
    username: &str,
    request: &PublishArtifactRequest,
) -> Result<PreparedPublication> {
    let validated = request.validate().map_err(|err| protocol_error(&err))?;
    let payload = validated.payload;
    let owner = owner_key(username, &payload.owner)?;
    let entry = entry_digest(&request.key, &payload.subject);
    let envelope_bytes = serde_json::to_vec(&request.envelope)?;
    // Named for what the artifact is *for* rather than what it is, so that one
    // input key and one set of compatibility constraints admit one artifact.
    let slot = compatibility_slot(&payload.compatibility);
    let started = std::time::Instant::now();
    let envelope_digest = request.envelope.digest().map_err(|err| protocol_error(&err))?;
    let variant_path = format!("{owner}/entries/{entry}/{slot}.json");
    Ok(PreparedPublication {
        started,
        payload,
        uploads: validated.blobs,
        owner,
        entry,
        envelope_digest,
        envelope_bytes,
        variant_path,
    })
}

/// An uploaded blob must be exactly what the signed manifest declares.
fn verify_upload(id: &str, integrity: &str, size: u64, upload: Option<&[u8]>) -> Result<()> {
    let Some(bytes) = upload else {
        return Ok(());
    };
    if bytes.len() as u64 != size {
        return Err(bad_request(format!(
            "blob {id} has {} bytes but the signed manifest declares {size}",
            bytes.len(),
        )));
    }
    verify_blob(integrity, bytes).map_err(|err| protocol_error(&err))
}

fn verify_stored_blob(id: &str, integrity: &str, size: u64, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 != size {
        return Err(RegistryError::Internal {
            reason: format!(
                "stored shared artifact blob {id} has {} bytes instead of {size}",
                bytes.len(),
            ),
        });
    }
    verify_blob(integrity, bytes).map_err(|err| RegistryError::Internal {
        reason: format!("stored shared artifact blob failed verification: {err}"),
    })
}

fn stored_object_too_large(size: u64, max_size: u64) -> RegistryError {
    RegistryError::Internal {
        reason: format!("stored shared artifact object has {size} bytes; limit is {max_size}"),
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

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

fn is_write_conflict(error: &object_store::Error) -> bool {
    matches!(
        error,
        object_store::Error::AlreadyExists { .. } | object_store::Error::Precondition { .. },
    )
}

fn is_create_conflict(error: &object_store::Error) -> bool {
    matches!(
        error,
        object_store::Error::AlreadyExists { .. } | object_store::Error::Precondition { .. },
    )
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

fn storage_quota_error() -> RegistryError {
    bad_request(format!(
        "shared artifact storage quota exceeded ({MAX_OWNER_ARTIFACT_BYTES} bytes per owner, {MAX_GLOBAL_ARTIFACT_BYTES} bytes globally)",
    ))
}

fn quota_counter_underflow() -> RegistryError {
    RegistryError::Internal { reason: "shared artifact quota counter underflow".to_string() }
}

fn protocol_error(error: &ArtifactProtocolError) -> RegistryError {
    bad_request(error.to_string())
}

fn bad_request(reason: String) -> RegistryError {
    RegistryError::BadRequest { reason }
}

#[cfg(test)]
mod tests;
