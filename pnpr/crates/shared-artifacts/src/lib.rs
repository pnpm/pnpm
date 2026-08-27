use std::{
    collections::{BTreeMap, HashSet},
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use futures_util::{StreamExt as _, stream::BoxStream};
use object_store::{
    ObjectMeta, ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion,
    local::LocalFileSystem, path::Path as ObjectPath,
};
use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, ArtifactCandidate, ArtifactPayload, ArtifactProtocolError,
    ArtifactVariant, MAX_CANDIDATES, MAX_FILE_SIZE, MAX_RESOLVE_RESPONSE_SIZE,
    MAX_VARIANTS_PER_CANDIDATE, OwnerScope, PackageIdentity, PublishArtifactRequest,
    ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact, SignedArtifactEnvelope,
    blob_id, verify_blob,
};
use pnpr_config::{HostedStoreConfig, build_s3_store, normalize_key_prefix};
use pnpr_error::{RegistryError, Result};
use sha2::{Digest as _, Sha256};
use tokio::time::sleep;

const ARTIFACT_CACHE_DIR: &str = "shared-artifacts/v0";
const ARTIFACT_OBJECT_PREFIX: &str = ".pnpr-artifacts/v0";
const ARTIFACT_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const ARTIFACT_USAGE_FILE: &str = ".locks/usage.json";
const ARTIFACT_QUOTA_OBJECT: &str = "quota.json";
const MAX_OWNER_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_GLOBAL_ARTIFACT_BYTES: u64 = 10 * MAX_OWNER_ARTIFACT_BYTES;
const QUOTA_WRITE_RETRIES: usize = 32;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct ArtifactUsage {
    global_bytes: u64,
    owner_bytes: BTreeMap<String, u64>,
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

/// Shared build-artifact storage. Local deployments use the
/// `cache/shared-artifacts/v0` layout. Object-store deployments use the same
/// configured bucket as hosted packages under a reserved namespace, allowing
/// every replica to observe the same immutable blobs and envelopes.
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

    fn object_store(store: Arc<dyn ObjectStore>, prefix: &str) -> Self {
        Self {
            store,
            prefix: format!("{prefix}{ARTIFACT_OBJECT_PREFIX}/"),
            quota: QuotaCoordination::Conditional,
            owner_limit: MAX_OWNER_ARTIFACT_BYTES,
            global_limit: MAX_GLOBAL_ARTIFACT_BYTES,
        }
    }

    pub async fn publish(&self, username: &str, request: PublishArtifactRequest) -> Result<bool> {
        let validated = request.validate().map_err(|err| protocol_error(&err))?;
        let payload = validated.payload;
        let mut uploads = validated.blobs;
        let owner = owner_key(username, &payload.owner)?;
        let entry = entry_digest(&request.key, &payload.package, &payload.source_integrity);
        let envelope = request.envelope.digest().map_err(|err| protocol_error(&err))?;
        let envelope_bytes = serde_json::to_vec(&request.envelope)?;
        let envelope_size = envelope_bytes.len() as u64;
        let variant_path = format!("{owner}/entries/{entry}/{envelope}.json");

        if self.object_exists(&variant_path).await? {
            return Ok(false);
        }

        let required: BTreeMap<&str, u64> = payload
            .manifest
            .added
            .iter()
            .map(|file| (file.integrity.as_str(), file.size))
            .collect();
        let mut new_blobs = Vec::new();
        for (integrity, size) in required {
            let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
            let path = format!("{owner}/blobs/{id}");
            let upload = uploads.remove(integrity);
            if let Some(bytes) = upload.as_deref() {
                if bytes.len() as u64 != size {
                    return Err(bad_request(format!(
                        "blob {id} has {} bytes but the signed manifest declares {size}",
                        bytes.len(),
                    )));
                }
                verify_blob(integrity, bytes).map_err(|err| protocol_error(&err))?;
            }
            match self.read_object_bounded(&path, size).await? {
                Some(bytes) => verify_stored_blob(&id, integrity, size, &bytes)?,
                None => match upload {
                    Some(bytes) => new_blobs.push((path, bytes)),
                    None => {
                        return Err(bad_request(format!(
                            "signed manifest references blob {id} without uploading it",
                        )));
                    }
                },
            }
        }

        let added_bytes = new_blobs.iter().try_fold(envelope_size, |total, entry| {
            total.checked_add(entry.1.len() as u64).ok_or_else(storage_quota_error)
        })?;
        self.reserve_quota(&owner, added_bytes).await?;

        let mut retained_bytes = 0_u64;
        for (path, bytes) in new_blobs {
            let size = bytes.len() as u64;
            match self.create_object(&path, bytes).await {
                Ok(true) => retained_bytes += size,
                Ok(false) => {}
                Err(error) => {
                    retained_bytes += size;
                    self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                    return Err(error);
                }
            }
        }
        let created = match self.create_object(&variant_path, envelope_bytes).await {
            Ok(created) => created,
            Err(error) => {
                retained_bytes += envelope_size;
                self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                return Err(error);
            }
        };
        if created {
            retained_bytes += envelope_size;
        }
        self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
        Ok(created)
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
        let entry = entry_digest(&candidate.key, &candidate.package, &candidate.source_integrity);
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

    async fn reserve_quota(&self, owner: &str, added_bytes: u64) -> Result<()> {
        self.change_quota(owner, added_bytes, QuotaChange::Reserve).await
    }

    async fn release_uncommitted(
        &self,
        owner: &str,
        reserved_bytes: u64,
        retained_bytes: u64,
    ) -> Result<()> {
        let unused_bytes =
            reserved_bytes.checked_sub(retained_bytes).ok_or_else(quota_counter_underflow)?;
        if unused_bytes != 0 {
            self.change_quota(owner, unused_bytes, QuotaChange::Release).await?;
        }
        Ok(())
    }

    async fn change_quota(&self, owner: &str, bytes: u64, change: QuotaChange) -> Result<()> {
        match &self.quota {
            QuotaCoordination::Local { lock_path } => {
                let _lock = acquire_artifact_lock(lock_path.clone()).await?;
                let (mut usage, _) = self.load_usage().await?;
                self.change_usage(&mut usage, owner, bytes, change)?;
                self.write_usage(&usage, PutMode::Overwrite).await?;
                Ok(())
            }
            QuotaCoordination::Conditional => {
                for _ in 0..QUOTA_WRITE_RETRIES {
                    let (mut usage, version) = self.load_usage().await?;
                    self.change_usage(&mut usage, owner, bytes, change)?;
                    let mode = version.map_or(PutMode::Create, PutMode::Update);
                    match self.write_usage(&usage, mode).await {
                        Ok(()) => return Ok(()),
                        Err(RegistryError::ObjectStore(error)) if is_write_conflict(&error) => {}
                        Err(error) => return Err(error),
                    }
                }
                Err(RegistryError::Internal {
                    reason: "shared artifact quota changed too often while updating storage"
                        .to_string(),
                })
            }
        }
    }

    fn change_usage(
        &self,
        usage: &mut ArtifactUsage,
        owner: &str,
        bytes: u64,
        change: QuotaChange,
    ) -> Result<()> {
        if matches!(change, QuotaChange::Release) {
            usage.global_bytes =
                usage.global_bytes.checked_sub(bytes).ok_or_else(quota_counter_underflow)?;
            let owner_bytes =
                usage.owner_bytes.get_mut(owner).ok_or_else(quota_counter_underflow)?;
            *owner_bytes = owner_bytes.checked_sub(bytes).ok_or_else(quota_counter_underflow)?;
            return Ok(());
        }
        let owner_bytes = usage.owner_bytes.get(owner).copied().unwrap_or(0);
        let next_owner = owner_bytes.checked_add(bytes).ok_or_else(storage_quota_error)?;
        let next_global = usage.global_bytes.checked_add(bytes).ok_or_else(storage_quota_error)?;
        if next_owner > self.owner_limit || next_global > self.global_limit {
            return Err(storage_quota_error());
        }
        usage.global_bytes = next_global;
        usage.owner_bytes.insert(owner.to_string(), next_owner);
        Ok(())
    }

    async fn load_usage(&self) -> Result<(ArtifactUsage, Option<UpdateVersion>)> {
        let path = self.object_path(self.quota_object());
        match self.store.get(&path).await {
            Ok(result) => {
                let version = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                let bytes = result.bytes().await?;
                Ok((serde_json::from_slice(&bytes)?, Some(version)))
            }
            Err(object_store::Error::NotFound { .. }) => Ok((self.scan_usage().await?, None)),
            Err(error) => Err(error.into()),
        }
    }

    async fn scan_usage(&self) -> Result<ArtifactUsage> {
        let mut usage = ArtifactUsage::default();
        for entry in self.list_objects(None).await? {
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            if relative == self.quota_object() || relative.starts_with(".locks/") {
                continue;
            }
            let Some((owner, _)) = relative.split_once('/') else { continue };
            let size = entry.size;
            usage.global_bytes =
                usage.global_bytes.checked_add(size).ok_or_else(storage_quota_error)?;
            let owner_bytes = usage.owner_bytes.entry(owner.to_string()).or_default();
            *owner_bytes = owner_bytes.checked_add(size).ok_or_else(storage_quota_error)?;
        }
        Ok(usage)
    }

    async fn write_usage(&self, usage: &ArtifactUsage, mode: PutMode) -> Result<()> {
        self.store
            .put_opts(
                &self.object_path(self.quota_object()),
                PutPayload::from(serde_json::to_vec(usage)?),
                PutOptions { mode, ..PutOptions::default() },
            )
            .await?;
        Ok(())
    }

    async fn object_exists(&self, relative: &str) -> Result<bool> {
        match self.store.head(&self.object_path(relative)).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    async fn create_object(&self, relative: &str, bytes: Vec<u8>) -> Result<bool> {
        match self
            .store
            .put_opts(
                &self.object_path(relative),
                PutPayload::from(bytes),
                PutOptions { mode: PutMode::Create, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(error) if is_create_conflict(&error) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    async fn read_object_bounded(&self, relative: &str, max_size: u64) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.object_path(relative)).await {
            Ok(result) => {
                if result.meta.size > max_size {
                    return Err(stored_object_too_large(result.meta.size, max_size));
                }
                Ok(Some(result.bytes().await?.to_vec()))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    async fn read_object_path(&self, path: &ObjectPath) -> Result<Option<Vec<u8>>> {
        match self.store.get(path).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    async fn list_objects(&self, relative_prefix: Option<&str>) -> Result<Vec<ObjectMeta>> {
        let prefix = relative_prefix.map(|prefix| self.object_path(prefix)).or_else(|| {
            (!self.prefix.is_empty()).then(|| ObjectPath::from(self.prefix.trim_end_matches('/')))
        });
        let mut listing = self.store.list(prefix.as_ref());
        let mut entries = Vec::new();
        while let Some(entry) = listing.next().await {
            entries.push(entry?);
        }
        Ok(entries)
    }

    fn object_path(&self, relative: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{relative}", self.prefix))
    }

    fn relative_path<'a>(&self, path: &'a ObjectPath) -> Option<&'a str> {
        path.as_ref().strip_prefix(&self.prefix)
    }

    fn quota_object(&self) -> &str {
        match &self.quota {
            QuotaCoordination::Local { .. } => ARTIFACT_USAGE_FILE,
            QuotaCoordination::Conditional => ARTIFACT_QUOTA_OBJECT,
        }
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

fn owner_key(username: &str, owner: &OwnerScope) -> Result<String> {
    match owner {
        OwnerScope::Organization { name } if name == username => {
            Ok(digest_segment(owner.namespace().as_bytes()))
        }
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

fn object_name(path: &ObjectPath) -> &str {
    path.as_ref().rsplit('/').next().unwrap_or_default()
}

fn is_variant_file(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 69
        && bytes[64..] == *b".json"
        && bytes[..64].iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
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
