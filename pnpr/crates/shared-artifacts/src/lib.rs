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
use sha2::{Digest as _, Sha256};
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
        let result = self
            .while_renewing(
                &publication,
                PUBLICATION_RENEWAL_INTERVAL,
                self.publish_active(prepared, &publication, &mut reclamation_needed),
            )
            .await;
        let finish = self.finish_publication(&publication, reclamation_needed).await;
        if finish.is_ok()
            && let Err(error) = self.try_reclaim_unreferenced_blobs().await
        {
            tracing::warn!(%error, "shared artifact reclamation failed");
        }
        finish?;
        result
    }

    /// Runs `work`, saying at intervals that the publication is still working.
    ///
    /// A registration is written off so that one nobody will remove stops
    /// holding reclamation shut. Renewing keeps that from reaching a
    /// publication that is merely slow: it takes every renewal across the
    /// expiry failing for a live one to go quiet long enough. That is why the
    /// recovery afterwards is not redundant — renewals can fail — but it is
    /// what makes needing it rare rather than ordinary.
    ///
    /// The renewals are a branch of the select rather than a handler around it,
    /// so `work` keeps being polled while a renewal waits. What a renewal waits
    /// for is the lock a local usage mutation holds, and the publication holding
    /// that lock is the one being renewed: handling renewals between polls would
    /// leave each waiting on the other for good.
    async fn while_renewing<Outcome>(
        &self,
        publication: &str,
        between_renewals: Duration,
        work: impl Future<Output = Outcome>,
    ) -> Outcome {
        let mut renewals = interval(between_renewals);
        renewals.tick().await;
        let renewing = async {
            loop {
                renewals.tick().await;
                // A renewal that cannot be written is not fatal on its own: the
                // expiry is several renewals wide, and the publication recovers
                // what it lost if it is written off anyway.
                if let Err(error) = self.renew_publication(publication).await {
                    tracing::warn!(%error, "shared artifact publication could not renew");
                }
            }
        };
        tokio::select! {
            outcome = work => outcome,
            () = renewing => unreachable!("renewals stop only when the publication does"),
        }
    }

    async fn renew_publication(&self, publication: &str) -> Result<()> {
        self.mutate_usage(|usage| {
            if !usage.active_publications.contains(publication) {
                return Ok(false);
            }
            usage.active_publication_times.insert(publication.to_string(), registered_now());
            Ok(true)
        })
        .await?;
        Ok(())
    }

    async fn publish_active(
        &self,
        prepared: PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
    ) -> Result<bool> {
        let (stored, created) =
            self.publish_claimed(prepared, publication, reclamation_needed).await;
        if stored.is_err() && !created.is_empty() {
            // The scopes stay claimed. Giving them back here cannot be ordered
            // against a publication of the same envelope, which recognises these
            // markers rather than creating its own and can store the artifact at
            // any point around the giving back — every arrangement of reading
            // and deleting leaves one interleaving that takes the scopes out
            // from under an artifact that is stored. Reclamation runs only when
            // no publication is in flight, so it can tell a scope no artifact
            // holds from one being claimed right now, and drops it there.
            *reclamation_needed = true;
        }
        stored
    }

    /// Publishes one artifact, reporting the scopes it reserved along with the
    /// outcome so a failure can give them back. The claim comes after the quota
    /// is reserved: a marker is an object like any other, and an owner over
    /// quota must not be able to write one.
    async fn publish_claimed(
        &self,
        prepared: PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
    ) -> (Result<bool>, Vec<String>) {
        let mut created = Vec::new();
        let stored =
            self.publish_reserving(prepared, publication, reclamation_needed, &mut created).await;
        (stored, created)
    }

    async fn publish_reserving(
        &self,
        prepared: PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        // Before reserving anything: a publication of an artifact already stored
        // and already holding its scopes writes nothing, so charging it for what
        // it will not store would refuse a retry an owner at their limit is
        // entitled to. Reads only, so nothing is written ahead of the quota.
        if self.publication_is_complete(&prepared).await? {
            return Ok(false);
        }
        let started = prepared.started;
        let mut prepared = prepared;
        let envelope_size = prepared.envelope_bytes.len() as u64;
        let owner = prepared.owner.clone();

        let required: BTreeMap<String, u64> = prepared
            .payload
            .manifest
            .added
            .iter()
            .map(|file| (file.integrity.clone(), file.size))
            .collect();
        let mut new_blobs = Vec::new();
        for (integrity, size) in required {
            let integrity: &str = &integrity;
            let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
            let path = format!("{owner}/blobs/{id}");
            let upload = prepared.uploads.remove(integrity);
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

        // The markers this publication is about to claim are objects like any
        // other, so an owner at their limit cannot write them either.
        let scopes = match compatibility_scopes(&prepared.payload.compatibility) {
            CompatibilityScopes::Every => 1,
            CompatibilityScopes::These(scopes) => scopes.len(),
        };
        let scope_bytes = (scopes as u64)
            .checked_mul(prepared.envelope_digest.len() as u64)
            .ok_or_else(storage_quota_error)?;
        let added_bytes = new_blobs
            .iter()
            .try_fold(envelope_size, |total, entry| {
                total.checked_add(entry.1.len() as u64).ok_or_else(storage_quota_error)
            })?
            .checked_add(scope_bytes)
            .ok_or_else(storage_quota_error)?;
        if let Err(error) = self.reserve_quota(&owner, added_bytes).await {
            *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }

        let mut retained_bytes = 0_u64;
        match self.claim_scopes(&prepared, created).await {
            Ok(SlotClaim::Held) => {
                self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                return Ok(false);
            }
            Ok(SlotClaim::HeldByAnother) => {
                self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                return Err(RegistryError::ArtifactAlreadyPublished {
                    owner,
                    entry: prepared.entry,
                });
            }
            // The markers this publication wrote, not the scopes it reaches: one
            // it found already its own was charged to whoever wrote it. They are
            // kept whatever becomes of the artifact, since only reclamation
            // gives a scope back.
            Ok(SlotClaim::Free) => {
                retained_bytes += (created.len() as u64) * prepared.envelope_digest.len() as u64;
            }
            Err(error) => {
                *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
                self.release_uncommitted(&owner, added_bytes, retained_bytes).await?;
                return Err(error);
            }
        }
        let PreparedPublication {
            entry,
            envelope_bytes,
            variant_path,
            payload,
            envelope_digest,
            ..
        } = prepared;
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
        // Read before the quota is released and inspected after, so that a
        // store error here cannot return while this publication is still
        // charged for an envelope it did not store — a leak that would
        // accumulate silently and eventually refuse publications that fit.
        let winner = if created {
            Ok(None)
        } else {
            self.read_object_bounded(&variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64).await
        };
        if let Err(error) = self.release_uncommitted(&owner, added_bytes, retained_bytes).await {
            *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }
        let winner = match winner {
            Ok(winner) => winner,
            Err(error) => {
                *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
                return Err(error);
            }
        };
        if !created && winner.is_none_or(|winner| winner != envelope_bytes) {
            return Err(RegistryError::ArtifactAlreadyPublished { owner, entry });
        }
        if created && started.elapsed() >= ACTIVE_PUBLICATION_EXPIRY {
            // Long enough to have been written off, which lets reclamation run
            // and give back scopes this publication was still holding. Its
            // artifact is stored now, so those scopes are its own again, and
            // taking them back is what keeps it from reaching machines nothing
            // says it reaches.
            // Asked for whatever the recovery finds: a collector that ran
            // beside this publication rebuilt the usage document from what it
            // could see, which was not yet everything this publication had
            // written, and a recovery that gives up leaves an artifact and
            // markers to collect.
            *reclamation_needed = true;
            // Registered again first, and only then does the recovery look.
            // Being written off is what let a collector run beside this
            // publication; registering again waits for one that is running and
            // keeps another from starting, so what the recovery reads is still
            // there when it returns. A check on its own could not do that.
            if let Err(error) = self.begin_publication(publication).await {
                // Without the registration nothing can be read and believed, so
                // the artifact is taken out unlooked-at rather than left to
                // stand for blobs a collector may already have taken. The
                // scopes it holds name an artifact that is no longer there,
                // which is what reclamation collects.
                self.store.delete(&self.object_path(&variant_path)).await?;
                return Err(error);
            }
            self.recover_after_expiry(&owner, &entry, &variant_path, &payload, &envelope_digest)
                .await?;
        }
        Ok(created)
    }

    /// Makes good what a publication that ran long enough to be written off may
    /// have lost while it was running.
    ///
    /// Being written off lets reclamation run beside it, and reclamation
    /// collects what nothing references — which, before this publication stored
    /// its envelope, includes the blobs it had uploaded. An envelope naming
    /// files that are not there is worse than no artifact, so they are read back
    /// before the scopes are, and the artifact is taken out if any is gone.
    ///
    /// A scope may have gone to an artifact published while this one was
    /// written off, and that artifact holds it. This one then has no claim on a
    /// machine it reaches, so it takes its own artifact back out rather than
    /// leaving two that reach it — the variant is named for constraints only an
    /// identical artifact shares, so removing it removes nothing else's.
    async fn recover_after_expiry(
        &self,
        owner: &str,
        entry: &str,
        variant_path: &str,
        payload: &ArtifactPayload,
        holder: &str,
    ) -> Result<()> {
        for file in &payload.manifest.added {
            let id = blob_id(&file.integrity).map_err(|err| protocol_error(&err))?;
            let path = format!("{owner}/blobs/{id}");
            let Some(bytes) = self.read_object_bounded(&path, file.size).await? else {
                self.store.delete(&self.object_path(variant_path)).await?;
                return Err(RegistryError::Internal {
                    reason: format!(
                        "blob {id} of a shared artifact was collected while the publication \
                         storing it was still running",
                    ),
                });
            };
            verify_stored_blob(&id, &file.integrity, file.size, &bytes)?;
        }
        let scopes = match compatibility_scopes(&payload.compatibility) {
            CompatibilityScopes::Every => BTreeSet::from([UNIVERSAL_SCOPE.to_string()]),
            CompatibilityScopes::These(scopes) => scopes,
        };
        let mut held = true;
        let mut retaken = Vec::new();
        for scope in &scopes {
            if self.create_object(&scope_marker_path(owner, entry, scope), holder.into()).await? {
                retaken.push(scope.clone());
                continue;
            }
            if self.scope_marker(owner, entry, scope, holder).await? != ScopeMarker::Ours {
                held = false;
                break;
            }
        }
        // The other form of the vocabulary reaches these machines too, and a
        // publication that took one while this was written off holds it under a
        // key this one never claims.
        if held {
            held = match compatibility_scopes(&payload.compatibility) {
                CompatibilityScopes::Every => self.tagged_scopes_are_free(owner, entry).await?,
                CompatibilityScopes::These(_) => {
                    self.scope_marker(owner, entry, UNIVERSAL_SCOPE, holder).await?
                        != ScopeMarker::Another
                }
            };
        }
        if held {
            return Ok(());
        }
        // The artifact goes first, and the scopes it retook after it. A store
        // error between the two leaves markers held for an artifact that is not
        // there, which refuses artifacts reaching those machines until
        // reclamation drops them; the other order would leave the artifact
        // resolvable while holding nothing, and one reaching the same machines
        // could be published beside it.
        self.store.delete(&self.object_path(variant_path)).await?;
        for scope in &retaken {
            // Only while it still names this artifact: a marker retaken here can
            // be collected and taken by somebody else before this loop reaches
            // it, and removing it by path alone would take that publication's
            // claim instead.
            if self.scope_marker(owner, entry, scope, holder).await? != ScopeMarker::Ours {
                continue;
            }
            let path = self.object_path(&scope_marker_path(owner, entry, scope));
            match self.store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(RegistryError::ArtifactAlreadyPublished {
            owner: owner.to_string(),
            entry: entry.to_string(),
        })
    }

    /// Whether nothing holds a scope named by a tag, which is what an artifact
    /// reaching every machine has to know: its own key says nothing about the
    /// keys tagged artifacts take. Stops at the first, and reads no artifact.
    async fn tagged_scopes_are_free(&self, owner: &str, entry: &str) -> Result<bool> {
        let prefix = self.object_path(&scopes_prefix(owner, entry));
        let mut listing = self.store.list(Some(&prefix));
        while let Some(marker) = listing.next().await {
            if scope_name(&marker?.location)
                .is_some_and(|scope| !matches!(scope, UNIVERSAL_SCOPE | BACKFILLED_SCOPE))
            {
                return Ok(false);
            }
        }
        Ok(true)
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

    /// Reserves every scope this artifact reaches, so no other artifact
    /// reaching any of them can be published beside it.
    ///
    /// Each scope is its own conditional create, and that is what orders two
    /// publications whose constraints merely overlap: they contend on the scope
    /// they share, rather than on a path that only identical constraints agree
    /// on. The work is proportional to this artifact's own tags — 64 at the
    /// most, one in practice — not to what the entry already holds, save for the
    /// single backfill an entry written before markers existed needs.
    async fn claim_scopes(
        &self,
        publication: &PreparedPublication,
        created: &mut Vec<String>,
    ) -> Result<SlotClaim> {
        let PreparedPublication {
            owner,
            entry,
            envelope_digest,
            variant_path,
            envelope_bytes,
            payload,
            ..
        } = publication;
        // The markers an entry needed are objects this publication wrote, and
        // they outlive it, so it carries them even though they name artifacts
        // somebody else stored.
        self.backfill_scopes(publication).await?;
        let claimed = match compatibility_scopes(&payload.compatibility) {
            CompatibilityScopes::Every => {
                self.claim_universal_scope(owner, entry, envelope_digest, created).await
            }
            CompatibilityScopes::These(scopes) => {
                self.claim_tagged_scopes(owner, entry, envelope_digest, &scopes, created).await
            }
        };
        let claimed = match claimed {
            Ok(claimed) => claimed,
            Err(error) => return Err(error),
        };
        if !claimed {
            return Ok(SlotClaim::HeldByAnother);
        }
        // The scopes belong to this artifact either way. Whether *this* envelope
        // is the one already stored for them is the variant's own question, and
        // a stored one under a different envelope means two builds share a slot.
        match self.read_object_bounded(variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64).await {
            Ok(Some(stored)) if &stored == envelope_bytes => Ok(SlotClaim::Held),
            Ok(Some(_)) => Ok(SlotClaim::HeldByAnother),
            Ok(None) => Ok(SlotClaim::Free),
            Err(error) => Err(error),
        }
    }

    /// Whether this artifact is stored and already holds every scope it reaches,
    /// which is what a retry of a publication that finished looks like.
    ///
    /// The variant is read first, so a publication of something not yet stored
    /// pays one read to find that out and stops.
    async fn publication_is_complete(&self, publication: &PreparedPublication) -> Result<bool> {
        let PreparedPublication {
            owner, entry, envelope_digest, variant_path, envelope_bytes, ..
        } = publication;
        if self
            .read_object_bounded(variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64)
            .await?
            .is_none_or(|stored| &stored != envelope_bytes)
        {
            return Ok(false);
        }
        let scopes = match compatibility_scopes(&publication.payload.compatibility) {
            CompatibilityScopes::Every => BTreeSet::from([UNIVERSAL_SCOPE.to_string()]),
            CompatibilityScopes::These(scopes) => scopes,
        };
        for scope in &scopes {
            if self.scope_marker(owner, entry, scope, envelope_digest).await? != ScopeMarker::Ours {
                return Ok(false);
            }
        }
        // Holding its own scopes is not enough: an entry can hold an artifact
        // reaching the same machines from the other side of the vocabulary, and
        // a retry into one of those is refused like any other publication rather
        // than reported as already published.
        match compatibility_scopes(&publication.payload.compatibility) {
            CompatibilityScopes::Every => {
                if !self.tagged_scopes_are_free(owner, entry).await? {
                    return Ok(false);
                }
            }
            CompatibilityScopes::These(_) => {
                if self.scope_marker(owner, entry, UNIVERSAL_SCOPE, envelope_digest).await?
                    == ScopeMarker::Another
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// A universal artifact reaches every scope, and cannot enumerate them to
    /// claim one by one. It takes the reserved key instead, then looks for a
    /// tagged scope it would have contended with — a listing that stops at the
    /// first one and reads nothing.
    async fn claim_universal_scope(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        if !self.claim_scope(owner, entry, UNIVERSAL_SCOPE, holder, created).await? {
            return Ok(false);
        }
        self.tagged_scopes_are_free(owner, entry).await
    }

    async fn claim_tagged_scopes(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        scopes: &BTreeSet<String>,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        for scope in scopes {
            if !self.claim_scope(owner, entry, scope, holder, created).await? {
                return Ok(false);
            }
        }
        // A universal artifact publishing at the same time claims its own key
        // rather than any of these, so reading it afterwards is what settles
        // which of the two arrived first. Nobody holding it is the ordinary
        // case, not a conflict.
        Ok(self.scope_marker(owner, entry, UNIVERSAL_SCOPE, holder).await? != ScopeMarker::Another)
    }

    /// Whether this artifact holds `scope`, having either created the marker or
    /// found one it had already taken. Recognising its own marker is what keeps
    /// republishing an artifact a retry rather than a conflict with itself.
    async fn claim_scope(
        &self,
        owner: &str,
        entry: &str,
        scope: &str,
        holder: &str,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        match self.create_object(&scope_marker_path(owner, entry, scope), holder.into()).await {
            Ok(true) => {
                created.push(scope.to_string());
                Ok(true)
            }
            // A marker that lost the create and then went is nobody's, this
            // artifact's least of all, so it is refused rather than assumed.
            Ok(false) => {
                Ok(self.scope_marker(owner, entry, scope, holder).await? == ScopeMarker::Ours)
            }
            Err(error) => {
                // The write can reach the store and still report failure, and a
                // marker nobody is tracking would refuse every later artifact
                // for this scope. Claiming it only when it turns out to hold
                // this artifact keeps the release from touching another's.
                if self
                    .scope_marker(owner, entry, scope, holder)
                    .await
                    .is_ok_and(|marker| marker == ScopeMarker::Ours)
                {
                    created.push(scope.to_string());
                }
                Err(error)
            }
        }
    }

    /// Who holds a scope, which absence does not answer on its own: another
    /// publication can release a marker between the create that lost and this
    /// read, and treating what is gone as this artifact's own would store it
    /// reserving nothing.
    async fn scope_marker(
        &self,
        owner: &str,
        entry: &str,
        scope: &str,
        holder: &str,
    ) -> Result<ScopeMarker> {
        Ok(
            match self
                .read_object_bounded(
                    &scope_marker_path(owner, entry, scope),
                    MAX_SCOPE_MARKER_BYTES,
                )
                .await?
            {
                None => ScopeMarker::Gone,
                Some(stored) if stored == holder.as_bytes() => ScopeMarker::Ours,
                Some(_) => ScopeMarker::Another,
            },
        )
    }

    /// Gives an entry whose artifacts hold no scopes the markers they reach, so
    /// that reading the markers speaks for everything stored.
    ///
    /// This is the one place that reads what an entry already holds, and a
    /// count cannot bound it the way one bounds a lookup: a variant it skipped
    /// would leave the scope that variant reaches unclaimed, which is the hole
    /// markers close. It runs once — an entry holding any marker is already
    /// described by them — and each read is bounded by the envelope limit.
    async fn backfill_scopes(&self, publication: &PreparedPublication) -> Result<()> {
        let PreparedPublication { owner, entry, .. } = publication;
        // The sentinel, not the markers: they are written one at a time and the
        // scan stops at the first store error, so a marker only says some
        // artifact was reached, while the sentinel says every one was.
        let done = scope_marker_path(owner, entry, BACKFILLED_SCOPE);
        if self.read_object_bounded(&done, MAX_SCOPE_MARKER_BYTES).await?.is_some() {
            return Ok(());
        }
        let prefix = self.object_path(&format!("{owner}/entries/{entry}/"));
        let mut listing = self.store.list(Some(&prefix));
        let mut variants = Vec::new();
        while let Some(variant) = listing.next().await {
            let variant = variant?;
            if is_variant_file(object_name(&variant.location)) {
                variants.push(variant.location);
            }
        }
        // Legacy variants can reach a scope another already reached — an overlap
        // the markers are being written to stop. Writing the marker once rather
        // than once per variant keeps a crowded entry from turning one backfill
        // into a reservation and a release for each repeat.
        let mut attempted = BTreeSet::new();
        for location in variants {
            let Some(relative) = self.relative_path(&location).map(str::to_string) else {
                continue;
            };
            let Some(bytes) =
                self.read_object_bounded(&relative, MAX_RESOLVE_RESPONSE_SIZE as u64).await?
            else {
                continue;
            };
            let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
                continue;
            };
            let Ok((payload, _)) = envelope.decode_payload() else { continue };
            let Ok(digest) = envelope.digest() else { continue };
            let scopes = match compatibility_scopes(&payload.compatibility) {
                CompatibilityScopes::Every => BTreeSet::from([UNIVERSAL_SCOPE.to_string()]),
                CompatibilityScopes::These(scopes) => scopes,
            };
            for scope in &scopes {
                if !attempted.insert(scope.clone()) {
                    continue;
                }
                // Reserved before it is written and kept afterwards, like every
                // marker: these outlive the publication that writes them, and an
                // owner over quota must not be able to write one either.
                let bytes = digest.len() as u64;
                self.reserve_quota(owner, bytes).await?;
                match self
                    .create_object(&scope_marker_path(owner, entry, scope), digest.as_str().into())
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => self.release_uncommitted(owner, bytes, 0).await?,
                    Err(error) => {
                        // A store error says nothing about whether the marker
                        // landed, so what is charged is settled by looking
                        // rather than assumed. Only a marker this write put
                        // there stays charged: one that is not there is nobody's
                        // to pay for, and one holding another digest is charged
                        // to whoever wrote it. A read that fails too leaves the
                        // charge standing, since letting storage outgrow a quota
                        // is the worse way to be wrong.
                        if self.scope_marker(owner, entry, scope, &digest).await?
                            != ScopeMarker::Ours
                        {
                            self.release_uncommitted(owner, bytes, 0).await?;
                        }
                        return Err(error);
                    }
                }
            }
        }
        self.create_object(&done, Vec::new()).await?;
        Ok(())
    }

    /// Settles which publications are still in flight, and writes that down.
    ///
    /// Deciding it inside the pass that goes on to refuse something would leave
    /// the decision unwritten whenever the refusal happens, so a registration
    /// nobody keeps would be re-stamped on every read and outlive every pass.
    async fn expire_publications(&self) -> Result<()> {
        self.mutate_usage(|usage| Ok(expire_stranded_publications(usage))).await?;
        Ok(())
    }

    async fn begin_publication(&self, publication: &str) -> Result<()> {
        self.expire_publications().await?;
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
                    // Already registered is not a fault: a publication written
                    // off while it was still working registers again before it
                    // looks at what it may have lost, and may find its own
                    // registration still there.
                    usage.active_publications.insert(publication.to_string());
                    usage
                        .active_publication_times
                        .insert(publication.to_string(), registered_now());
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
                    // A registration missing here is not a fault: an expiry pass
                    // can write one off. What must not be lost is the request to
                    // reclaim, since the scopes a failed publication claimed come
                    // back only that way.
                    usage.active_publications.remove(publication);
                    usage.active_publication_times.remove(publication);
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
                        // Gone, and the reclamation this publication asked for
                        // already recorded: whether this attempt wrote that or
                        // an earlier one did, there is nothing left to do.
                        Ok((usage, _))
                            if !usage.active_publications.contains(publication)
                                && (!reclamation_needed || usage.reclamation_needed) =>
                        {
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
        self.expire_publications().await?;
        let reclamation = artifact_operation_id()?;
        let acquired = match self
            .mutate_usage(|usage| {
                // Dropped here rather than merely disregarded, so that the
                // check on completion sees a publication that started during
                // this run rather than one this run decided to ignore.
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
        let artifacts = self.referenced_blobs().await?;
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            if is_blob_path(relative) && !artifacts.referenced_blobs.contains(relative) {
                self.store.delete(&entry.location).await?;
                continue;
            }
            // Only when every variant was read: one that was not is stored all
            // the same, and dropping the scopes it holds would let an artifact
            // reaching the same machines be published beside it.
            if artifacts.every_variant_read
                && self.scope_is_abandoned(&entry.location, &artifacts.digests).await?
            {
                self.store.delete(&entry.location).await?;
            }
        }
        self.scan_usage().await
    }

    /// Whether a scope marker names an artifact that was never stored, which is
    /// what a publication that claimed the scope and then failed leaves behind.
    ///
    /// Only reclamation asks: it runs when no publication is in flight, so a
    /// marker with no artifact is abandoned rather than one being claimed at
    /// this moment. A publication cannot tell those apart, which is why it
    /// leaves its own scopes claimed and asks for reclamation instead.
    async fn scope_is_abandoned(
        &self,
        location: &ObjectPath,
        stored_artifacts: &HashSet<String>,
    ) -> Result<bool> {
        let Some(scope) = scope_name(location) else { return Ok(false) };
        if scope == BACKFILLED_SCOPE {
            return Ok(false);
        }
        let Some(relative) = self.relative_path(location).map(str::to_string) else {
            return Ok(false);
        };
        let Some(holder) = self.read_object_bounded(&relative, MAX_SCOPE_MARKER_BYTES).await?
        else {
            return Ok(false);
        };
        Ok(String::from_utf8(holder).is_ok_and(|holder| !stored_artifacts.contains(&holder)))
    }

    /// The blobs stored artifacts reference, the artifacts themselves, and
    /// whether every variant was read — all from one pass, since reclamation
    /// asks all three of every envelope it reads.
    ///
    /// A variant it could not read is stored all the same, and its scopes are
    /// its own. Saying so is what keeps a marker it holds from looking
    /// abandoned.
    async fn referenced_blobs(&self) -> Result<StoredArtifacts> {
        let mut artifacts = StoredArtifacts::default();
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            let Some(owner) = entry_owner(relative) else { continue };
            let variant = is_variant_file(object_name(&entry.location));
            if entry.size > MAX_RESOLVE_RESPONSE_SIZE as u64 {
                artifacts.every_variant_read &= !variant;
                continue;
            }
            let Some(bytes) = self.read_object_path(&entry.location).await? else {
                continue;
            };
            let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
                artifacts.every_variant_read &= !variant;
                continue;
            };
            let Ok((payload, _)) = envelope.decode_payload() else {
                artifacts.every_variant_read &= !variant;
                continue;
            };
            if digest_segment(payload.owner.namespace().as_bytes()) != owner {
                continue;
            }
            if variant {
                match envelope.digest() {
                    Ok(digest) => {
                        artifacts.digests.insert(digest);
                    }
                    Err(_) => artifacts.every_variant_read = false,
                }
            }
            for file in payload.manifest.added {
                let Ok(id) = blob_id(&file.integrity) else { continue };
                artifacts.referenced_blobs.insert(format!("{owner}/blobs/{id}"));
            }
        }
        Ok(artifacts)
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

fn scopes_prefix(owner: &str, entry: &str) -> String {
    format!("{owner}/entries/{entry}/scopes/")
}

fn scope_marker_path(owner: &str, entry: &str, scope: &str) -> String {
    format!("{}{scope}", scopes_prefix(owner, entry))
}

/// The scope a marker under an entry names, or `None` for anything else stored
/// there. Variants sit beside the marker directory, not inside it.
fn scope_name(path: &ObjectPath) -> Option<&str> {
    let (parent, name) = path.as_ref().rsplit_once('/')?;
    parent.ends_with("/scopes").then_some(name)
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

fn registered_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Drops publications that registered longer ago than a publication can
/// plausibly take, reporting whether it changed anything.
///
/// A publication with no registration time was registered by a replica that
/// does not keep them, so it is stamped now rather than written off: it may be
/// in flight, and expiring a live publication lets a collector run beside it.
/// The stamp is what a later pass measures against, which is why the caller
/// persists this whether or not anything was dropped.
fn expire_stranded_publications(usage: &mut ArtifactUsage) -> bool {
    let now = registered_now();
    let expiry = now.saturating_sub(ACTIVE_PUBLICATION_EXPIRY.as_secs());
    let before = usage.active_publications.len();
    let times = std::mem::take(&mut usage.active_publication_times);
    let mut stamped = false;
    usage.active_publication_times = usage
        .active_publications
        .iter()
        .map(|publication| {
            let registered = times.get(publication).copied().unwrap_or_else(|| {
                stamped = true;
                now
            });
            (publication.clone(), registered)
        })
        .collect();
    usage
        .active_publications
        .retain(|publication| usage.active_publication_times[publication] > expiry);
    usage
        .active_publication_times
        .retain(|publication, _| usage.active_publications.contains(publication));
    stamped
        || usage.active_publications.len() != before
        || times.len() != usage.active_publication_times.len()
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
/// envelope digests share.
fn compatibility_slot(compatibility: &CompatibilityConstraints) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pnpm-shared-artifact-slot-v1\0");
    match compatibility {
        CompatibilityConstraints::Universal => hasher.update(b"universal\0"),
        CompatibilityConstraints::Tagged { tags } => {
            hasher.update(b"tagged\0");
            // Sorted because matching a tag set is order-independent: two
            // orderings are the same constraint, and hashing them apart would
            // hand the same platform two slots to be published into. The
            // protocol already rejects duplicates, so sorting canonicalizes.
            let mut tags: Vec<&str> = tags.iter().map(String::as_str).collect();
            tags.sort_unstable();
            for tag in tags {
                hasher.update(tag.as_bytes());
                hasher.update([0]);
            }
        }
    }
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
