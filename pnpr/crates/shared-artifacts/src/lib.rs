use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
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
    ArtifactSubject, ArtifactVariant, CompatibilityConstraints, MAX_CANDIDATES, MAX_FILE_SIZE,
    MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE, OwnerScope, PublishArtifactRequest,
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
const MAX_ACTIVE_PUBLICATIONS: usize = 1024;
const PUBLICATION_FINISH_RETRIES: usize = 8;
const QUOTA_WRITE_RETRIES: usize = 32;
const RECLAMATION_WAIT_RETRIES: usize = 600;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct ArtifactUsage {
    global_bytes: u64,
    owner_bytes: BTreeMap<String, u64>,
    #[serde(default)]
    active_publications: BTreeSet<String>,
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

struct PreparedPublication {
    entry: String,
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
        let prepared = prepare_publication(username, &request)?;
        let publication = artifact_operation_id()?;
        self.begin_publication(&publication).await?;
        let mut reclamation_needed = false;
        let result = self.publish_active(prepared, &mut reclamation_needed).await;
        let finish = self.finish_publication(&publication, reclamation_needed).await;
        if finish.is_ok()
            && let Err(error) = self.try_reclaim_unreferenced_blobs().await
        {
            tracing::warn!(%error, "shared artifact reclamation failed");
        }
        finish?;
        result
    }

    async fn publish_active(
        &self,
        prepared: PreparedPublication,
        reclamation_needed: &mut bool,
    ) -> Result<bool> {
        let PreparedPublication {
            payload,
            mut uploads,
            owner,
            entry,
            envelope_bytes,
            variant_path,
        } = prepared;
        let envelope_size = envelope_bytes.len() as u64;

        // Idempotent for the identical envelope — a retried publication is not
        // an attempt to replace anything — and a conflict for any other, which
        // is the whole point of the slot. Releasing a claimed slot is an
        // operator action against the store: the publishing credential must
        // not be able to do it, or a stolen one could swap the artifact for a
        // dependency nobody has looked at in a year.
        if let Some(existing) =
            self.claimant(&owner, &entry, &variant_path, &payload.compatibility).await?
        {
            if existing == envelope_bytes {
                return Ok(false);
            }
            return Err(RegistryError::ArtifactAlreadyPublished { owner, entry });
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
        if let Err(error) = self.reserve_quota(&owner, added_bytes).await {
            *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }

        let mut retained_bytes = 0_u64;
        for (path, bytes) in new_blobs {
            let size = bytes.len() as u64;
            match self.create_object(&path, bytes).await {
                Ok(true) => retained_bytes += size,
                Ok(false) => {}
                Err(error) => {
                    *reclamation_needed = true;
                    retained_bytes += size;
                    self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                    return Err(error);
                }
            }
        }
        let created = match self.create_object(&variant_path, envelope_bytes.clone()).await {
            Ok(created) => created,
            Err(error) => {
                *reclamation_needed = true;
                retained_bytes += envelope_size;
                self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                return Err(error);
            }
        };
        if created {
            retained_bytes += envelope_size;
        }
        // Two publications can both find the slot empty above, so losing the
        // create is not by itself an idempotent retry: whoever won may have
        // stored something else, and reporting success would tell a publisher
        // its artifact is the one being served when it is not.
        let lost_to_other_bytes = !created
            && self
                .read_object_bounded(&variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64)
                .await?
                .is_none_or(|winner| winner != envelope_bytes);
        if let Err(error) = self.release_uncommitted(&owner, added_bytes, retained_bytes).await {
            *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }
        if lost_to_other_bytes {
            return Err(RegistryError::ArtifactAlreadyPublished { owner, entry });
        }
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

    /// The envelope already occupying this slot, if any.
    ///
    /// Usually that is the slot's own object. A store written before artifacts
    /// were named for their slot holds them under their envelope digest
    /// instead, so those are found by reading the entry — otherwise upgrading a
    /// populated registry would leave every existing artifact replaceable.
    async fn claimant(
        &self,
        owner: &str,
        entry: &str,
        variant_path: &str,
        compatibility: &CompatibilityConstraints,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(claimed) =
            self.read_object_bounded(variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64).await?
        {
            return Ok(Some(claimed));
        }
        let prefix = self.object_path(&format!("{owner}/entries/{entry}/"));
        let mut listing = self.store.list(Some(&prefix));
        let mut scanned = 0;
        while scanned < MAX_VARIANTS_PER_CANDIDATE {
            let Some(stored) = listing.next().await else { break };
            let stored = stored?;
            if !is_variant_file(object_name(&stored.location)) {
                continue;
            }
            scanned += 1;
            let Some(bytes) = self.read_object_path(&stored.location).await? else { continue };
            let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
                continue;
            };
            let Ok((payload, _)) = envelope.decode_payload() else { continue };
            if &payload.compatibility == compatibility {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    async fn begin_publication(&self, publication: &str) -> Result<()> {
        for _ in 0..RECLAMATION_WAIT_RETRIES {
            let begun = match self
                .mutate_usage(|usage| {
                    if usage.reclamation.is_some() {
                        return Ok(false);
                    }
                    if usage.active_publications.len() >= MAX_ACTIVE_PUBLICATIONS {
                        return Err(RegistryError::Internal {
                            reason: "shared artifact publication concurrency limit reached"
                                .to_string(),
                        });
                    }
                    if !usage.active_publications.insert(publication.to_string()) {
                        return Err(RegistryError::Internal {
                            reason: "shared artifact publication is already active".to_string(),
                        });
                    }
                    Ok(true)
                })
                .await
            {
                Ok(begun) => begun,
                Err(error) => {
                    if self.load_usage().await?.0.active_publications.contains(publication) {
                        return Ok(());
                    }
                    return Err(error);
                }
            };
            if begun {
                return Ok(());
            }
            sleep(ARTIFACT_LOCK_POLL_INTERVAL).await;
        }
        Err(RegistryError::Internal {
            reason: "shared artifact reclamation did not finish before publication timed out"
                .to_string(),
        })
    }

    async fn finish_publication(&self, publication: &str, reclamation_needed: bool) -> Result<()> {
        for attempt in 0..PUBLICATION_FINISH_RETRIES {
            let finished = self
                .mutate_usage(|usage| {
                    if !usage.active_publications.remove(publication) {
                        return Err(RegistryError::Internal {
                            reason: "shared artifact publication is not registered as active"
                                .to_string(),
                        });
                    }
                    usage.reclamation_needed |= reclamation_needed;
                    Ok(true)
                })
                .await;
            match finished {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    return Err(RegistryError::Internal {
                        reason: "shared artifact publication finish did not update usage"
                            .to_string(),
                    });
                }
                Err(error) => {
                    let retry_error = match self.load_usage().await {
                        Ok((usage, _)) if !usage.active_publications.contains(publication) => {
                            return Ok(());
                        }
                        Ok(_) => error,
                        Err(read_error) => read_error,
                    };
                    if attempt + 1 == PUBLICATION_FINISH_RETRIES {
                        return Err(retry_error);
                    }
                    sleep(quota_write_retry_delay(attempt)).await;
                }
            }
        }
        unreachable!("publication finish loop returns on its final attempt")
    }

    async fn try_reclaim_unreferenced_blobs(&self) -> Result<()> {
        let reclamation = artifact_operation_id()?;
        let acquired = match self
            .mutate_usage(|usage| {
                if !usage.reclamation_needed
                    || !usage.active_publications.is_empty()
                    || usage.reclamation.is_some()
                {
                    return Ok(false);
                }
                usage.reclamation = Some(reclamation.clone());
                Ok(true)
            })
            .await
        {
            Ok(acquired) => acquired,
            Err(error) => {
                if self.load_usage().await?.0.reclamation.as_deref() == Some(reclamation.as_str()) {
                    true
                } else {
                    return Err(error);
                }
            }
        };
        if !acquired {
            return Ok(());
        }

        match self.reclaim_unreferenced_blobs().await {
            Ok(usage) => {
                if let Err(error) = self.complete_reclamation(&reclamation, usage).await {
                    self.abort_reclamation(&reclamation).await?;
                    return Err(error);
                }
            }
            Err(error) => {
                self.abort_reclamation(&reclamation).await?;
                return Err(error);
            }
        }
        Ok(())
    }

    async fn reclaim_unreferenced_blobs(&self) -> Result<ArtifactUsage> {
        let referenced_blobs = self.referenced_blobs().await?;
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            if is_blob_path(relative) && !referenced_blobs.contains(relative) {
                self.store.delete(&entry.location).await?;
            }
        }
        self.scan_usage().await
    }

    async fn referenced_blobs(&self) -> Result<HashSet<String>> {
        let mut referenced = HashSet::new();
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            let Some(owner) = entry_owner(relative) else { continue };
            if entry.size > MAX_RESOLVE_RESPONSE_SIZE as u64 {
                continue;
            }
            let Some(bytes) = self.read_object_path(&entry.location).await? else {
                continue;
            };
            let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
                continue;
            };
            let Ok((payload, _)) = envelope.decode_payload() else {
                continue;
            };
            if digest_segment(payload.owner.namespace().as_bytes()) != owner {
                continue;
            }
            for file in payload.manifest.added {
                let Ok(id) = blob_id(&file.integrity) else { continue };
                referenced.insert(format!("{owner}/blobs/{id}"));
            }
        }
        Ok(referenced)
    }

    async fn complete_reclamation(
        &self,
        reclamation: &str,
        mut rebuilt: ArtifactUsage,
    ) -> Result<()> {
        rebuilt.reclamation = None;
        rebuilt.reclamation_needed = false;
        let changed = self
            .mutate_usage(|usage| {
                if usage.reclamation.as_deref() != Some(reclamation) {
                    return Err(RegistryError::Internal {
                        reason: "shared artifact reclamation ownership changed".to_string(),
                    });
                }
                if !usage.active_publications.is_empty() {
                    return Err(RegistryError::Internal {
                        reason: "shared artifact publication started during reclamation"
                            .to_string(),
                    });
                }
                *usage = rebuilt.clone();
                Ok(true)
            })
            .await?;
        if !changed {
            return Err(RegistryError::Internal {
                reason: "shared artifact reclamation did not update usage".to_string(),
            });
        }
        Ok(())
    }

    async fn abort_reclamation(&self, reclamation: &str) -> Result<()> {
        self.mutate_usage(|usage| {
            if usage.reclamation.as_deref() != Some(reclamation) {
                return Ok(false);
            }
            usage.reclamation = None;
            usage.reclamation_needed = true;
            Ok(true)
        })
        .await?;
        Ok(())
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
        let changed = self
            .mutate_usage(|usage| {
                self.change_usage(usage, owner, bytes, change)?;
                Ok(true)
            })
            .await?;
        if !changed {
            return Err(RegistryError::Internal {
                reason: "shared artifact quota update did not change usage".to_string(),
            });
        }
        Ok(())
    }

    async fn mutate_usage(
        &self,
        mutation: impl Fn(&mut ArtifactUsage) -> Result<bool>,
    ) -> Result<bool> {
        match &self.quota {
            QuotaCoordination::Local { lock_path } => {
                let _lock = acquire_artifact_lock(lock_path.clone()).await?;
                let (mut usage, _) = self.load_usage().await?;
                if !mutation(&mut usage)? {
                    return Ok(false);
                }
                self.write_usage(&usage, PutMode::Overwrite).await?;
                Ok(true)
            }
            QuotaCoordination::Conditional => {
                for attempt in 0..QUOTA_WRITE_RETRIES {
                    let (mut usage, version) = self.load_usage().await?;
                    if !mutation(&mut usage)? {
                        return Ok(false);
                    }
                    let mode = version.map_or(PutMode::Create, PutMode::Update);
                    match self.write_usage(&usage, mode).await {
                        Ok(()) => return Ok(true),
                        Err(RegistryError::ObjectStore(error)) if is_write_conflict(&error) => {
                            sleep(quota_write_retry_delay(attempt)).await;
                        }
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
            Err(object_store::Error::NotFound { .. }) => {
                let mut usage = self.scan_usage().await?;
                usage.reclamation_needed = usage.global_bytes != 0;
                Ok((usage, None))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn scan_usage(&self) -> Result<ArtifactUsage> {
        let mut usage = ArtifactUsage::default();
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
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

    fn list_objects(
        &self,
        relative_prefix: Option<&str>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        let prefix = relative_prefix.map(|prefix| self.object_path(prefix)).or_else(|| {
            (!self.prefix.is_empty()).then(|| ObjectPath::from(self.prefix.trim_end_matches('/')))
        });
        self.store.list(prefix.as_ref())
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
    // input key and one set of compatibility constraints admit one artifact and
    // a second build for the same input collides instead of joining it.
    let slot = compatibility_slot(&payload.compatibility);
    let variant_path = format!("{owner}/entries/{entry}/{slot}.json");
    Ok(PreparedPublication {
        payload,
        uploads: validated.blobs,
        owner,
        entry,
        envelope_bytes,
        variant_path,
    })
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

fn is_blob_path(relative: &str) -> bool {
    let mut segments = relative.split('/');
    let (Some(owner), Some("blobs"), Some(blob), None) =
        (segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return false;
    };
    is_digest_segment(owner) && !blob.is_empty()
}

fn entry_owner(relative: &str) -> Option<&str> {
    let mut segments = relative.split('/');
    let (Some(owner), Some("entries"), Some(entry), Some(variant), None) =
        (segments.next(), segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    (is_digest_segment(owner) && is_digest_segment(entry) && is_variant_file(variant))
        .then_some(owner)
}

fn is_digest_segment(segment: &str) -> bool {
    segment.len() == 64
        && segment.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn quota_write_retry_delay(attempt: usize) -> Duration {
    let base = 1_u64 << attempt.min(6);
    let mut random = [0_u8; 1];
    let jitter = if getrandom::fill(&mut random).is_ok() { u64::from(random[0]) % base } else { 0 };
    Duration::from_millis(base + jitter)
}

fn artifact_operation_id() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| RegistryError::Internal {
        reason: format!("could not generate a shared artifact operation ID: {error}"),
    })?;
    Ok(hex(&bytes))
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

/// The slot an artifact claims within its entry: one per set of compatibility
/// constraints, so a `universal` build and a glibc-2.31 build coexist while two
/// builds advertising the same constraints do not.
///
/// Hex-encoded so that it has the shape [`is_variant_file`] recognises, which
/// is also the shape of the envelope digests a store written before slots
/// existed used for these files.
fn compatibility_slot(compatibility: &CompatibilityConstraints) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-shared-artifact-slot-v1\0");
    hasher.update(serde_json::to_vec(compatibility).expect("compatibility constraints serialize"));
    hex(&hasher.finalize())
}

fn entry_digest(key: &str, subject: &ArtifactSubject) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-shared-artifact-entry-v1\0");
    hasher.update(key.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(subject).expect("artifact subjects serialize"));
    hex(&hasher.finalize())
}

fn artifact_matches_candidate(payload: &ArtifactPayload, candidate: &ArtifactCandidate) -> bool {
    let ArtifactCandidate { key: input_key, subject, owner } = candidate;
    payload.input_key == *input_key && payload.subject == *subject && payload.owner == *owner
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
