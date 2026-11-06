//! `PUT` and `HEAD /-/pnpr/v0/artifacts/blob?integrity=<sha512-...>` with no
//! owner: stage a release archive by integrity ahead of the publish commit
//! that references it
//! ([atomic workspace publication](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-atomic-workspace-publication.md)).
//!
//! The same route with an `organization` owner stores a shared build
//! artifact's blob instead (see [`super::pnpr_protocol`]); [`serve_put_blob`]
//! and [`serve_head_blob`] choose between the two by whether the query names
//! an owner.
//!
//! A staged archive belongs to the principal that uploaded it, and only a
//! commit by that principal can claim it. Nothing is published by staging:
//! the commit authorizes every package it publishes as an inline publish
//! would, and copies the archive into its package before it commits.

use super::{
    AppState, AuthedCaller, Body, Identity, MAX_TARBALL_BYTES, Response, State, StatusCode, header,
    private_no_cache,
};
use axum::{
    http::{HeaderMap, Uri},
    response::IntoResponse,
};
use futures_util::StreamExt as _;
use pnpm_shared_artifact_protocol::{blob_id, verify_blob_digest};
use pnpr_error::RegistryError;
use pnpr_storage::{StagedArchiveKey, Storage};
use sha2::{Digest as _, Sha512};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The most a principal may hold staged at once, across all its archives.
/// Archives of a release wait here only until its commit, so this is many
/// releases' worth.
pub(super) const MAX_STAGED_BYTES_PER_PRINCIPAL: u64 = 2 * 1024 * 1024 * 1024;

/// The most archives a principal may hold staged at once. Each upload of a
/// new archive reads every record of its principal, so this bounds that
/// work as well as the store.
pub(super) const MAX_STAGED_ARCHIVES_PER_PRINCIPAL: usize = 256;

/// Uploads received at once across the server. Each is buffered whole, up to
/// one archive's limit, so this bounds their memory. Another upload waits
/// for a permit rather than failing, so a publisher's parallel uploads queue.
pub(super) const MAX_CONCURRENT_STAGED_UPLOADS: usize = 4;

/// Uploads one principal may have received at once, of the server's
/// [`MAX_CONCURRENT_STAGED_UPLOADS`].
const MAX_CONCURRENT_STAGED_UPLOADS_PER_PRINCIPAL: usize = 2;

/// A semaphore per principal with an upload in flight, bounding how many of
/// the server's upload permits one principal holds. A principal's entry goes
/// when its last upload does.
#[derive(Default)]
pub(super) struct PrincipalUploads(Mutex<HashMap<String, Arc<Semaphore>>>);

impl PrincipalUploads {
    /// Wait for one of `principal`'s upload permits.
    async fn acquire(&self, principal: &str) -> PrincipalUpload<'_> {
        let semaphore = Arc::clone(
            self.0
                .lock()
                .expect("the principal upload map is never poisoned")
                .entry(principal.to_string())
                .or_insert_with(|| {
                    Arc::new(Semaphore::new(MAX_CONCURRENT_STAGED_UPLOADS_PER_PRINCIPAL))
                }),
        );
        let permit = Arc::clone(&semaphore)
            .acquire_owned()
            .await
            .expect("a principal's upload semaphore is never closed");
        PrincipalUpload { uploads: self, principal: principal.to_string(), permit: Some(permit) }
    }
}

/// One principal's upload permit, which removes the principal's entry when
/// it is the last one using it.
struct PrincipalUpload<'a> {
    uploads: &'a PrincipalUploads,
    principal: String,
    permit: Option<OwnedSemaphorePermit>,
}

impl Drop for PrincipalUpload<'_> {
    fn drop(&mut self) {
        drop(self.permit.take());
        let mut uploads =
            self.uploads.0.lock().expect("the principal upload map is never poisoned");
        // Only the map holds it: no upload is in flight or waiting.
        if uploads
            .get(&self.principal)
            .is_some_and(|semaphore| Arc::strong_count(semaphore) == 1)
        {
            uploads.remove(&self.principal);
        }
    }
}

/// The slowest upload rate a staged archive is given time for.
const MIN_UPLOAD_BYTES_PER_SECOND: u64 = 256 * 1024;

/// The most an upload reserves before its bytes arrive.
const INITIAL_BUFFER_BYTES: u64 = 1024 * 1024;

/// `PUT /-/pnpr/v0/artifacts/blob`: a release archive without an owner, a
/// shared artifact's blob with one.
pub(super) async fn serve_put_blob(
    state: State<AppState>,
    caller: AuthedCaller,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if names_owner(&uri) {
        if let Err(response) = artifact_caller(&state, &headers).await {
            return response;
        }
        return super::serve_upload_artifact_blob(state, caller, uri, headers, body).await;
    }
    if !state.inner.config.features.registry.enabled {
        return private_no_cache(StatusCode::NOT_FOUND.into_response());
    }
    private_no_cache(
        stage_archive(&state, &caller.0, &uri, &headers, body).await
            .map_or_else(IntoResponse::into_response, |created| {
                if created { StatusCode::CREATED } else { StatusCode::OK }.into_response()
            }),
    )
}

/// `HEAD /-/pnpr/v0/artifacts/blob`: whether a release archive is staged
/// without an owner, whether a shared artifact's blob is stored with one.
pub(super) async fn serve_head_blob(
    state: State<AppState>,
    caller: AuthedCaller,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if names_owner(&uri) {
        if let Err(response) = artifact_caller(&state, &headers).await {
            return response;
        }
        return super::serve_artifact_blob_size(state, caller, uri).await;
    }
    if !state.inner.config.features.registry.enabled {
        return private_no_cache(StatusCode::NOT_FOUND.into_response());
    }
    let result = async {
        let principal = principal(&caller.0)?;
        let digest = query_digest(&uri)?;
        state.inner.storage.staged_archive_size(&StagedArchiveKey { principal, digest: &digest })
            .await
    }
    .await;
    private_no_cache(match result {
        Ok(Some(size)) => ([(header::CONTENT_LENGTH, size.to_string())], ()).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => err.into_response(),
    })
}

/// Admit a shared artifact's blob request as the artifact surface does for
/// its other routes: only when the surface is served, and only for a caller
/// its protocol credentials identify. The release archive routes skip this
/// and accept the registry's credentials.
async fn artifact_caller(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    if state.inner.builds.artifacts.is_none() {
        return Err(private_no_cache(StatusCode::NOT_FOUND.into_response()));
    }
    match super::request_access::caller_username(state, headers).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            Err(RegistryError::Unauthenticated { resource: "shared artifacts".to_string() }
                .into_response())
        }
        Err(error) => Err(error.into_response()),
    }
}

/// Stage the archive streamed in `body` for the caller. `Ok(false)` when the
/// caller had it staged already, which renews it.
async fn stage_archive(
    state: &AppState,
    identity: &Identity,
    uri: &Uri,
    headers: &HeaderMap,
    body: Body,
) -> Result<bool, RegistryError> {
    let principal = principal(identity)?;
    let integrity = query_integrity(uri)?;
    let digest = archive_digest(&integrity)?;
    let size = declared_size(headers)?;
    let storage = &state.inner.storage;
    let key = StagedArchiveKey { principal, digest: &digest };
    // The principal's own permit first, so its uploads queue behind each
    // other rather than taking every permit of the server.
    let _principal_upload = state.inner.locks
        .staged_archive_uploads_by_principal
        .acquire(principal)
        .await;
    let _upload = state.inner.locks.staged_archive_uploads
        .acquire()
        .await
        .expect("the staged upload semaphore is never closed");
    // Uploading an archive staged already only renews it. Any other upload
    // reserves its size first, so uploads running in parallel see each
    // other's when they check the limits. The usage check counts only held
    // archives, so a near-expiry archive being renewed and expired records
    // do not block it.
    let renewing = storage.staged_archive_size(&key).await?.is_some()
        || !reserve_within_limits(storage, &key, size).await?;
    let received =
        tokio::time::timeout(receive_deadline(size), receive_archive(body, &integrity, size))
            .await
            .unwrap_or_else(|_| {
                Err(bad_request("the archive upload was too slow to finish in time".to_string()))
            });
    let staged = match received {
        Ok(bytes) => storage.stage_archive(&key, bytes).await,
        Err(err) => Err(err),
    };
    if staged.is_err() && !renewing {
        storage.release_staged_archive(&key).await?;
    }
    staged
}

/// How long receiving an archive of `size` bytes may take while it holds an
/// upload permit: a minute, plus the time to send it at
/// [`MIN_UPLOAD_BYTES_PER_SECOND`]. A client that withholds its bytes would
/// otherwise keep other publishers waiting for the permit.
fn receive_deadline(size: u64) -> std::time::Duration {
    std::time::Duration::from_secs(60 + size / MIN_UPLOAD_BYTES_PER_SECOND)
}

/// The `Content-Length` of a staged archive upload, within one archive's
/// limit.
fn declared_size(headers: &HeaderMap) -> Result<u64, RegistryError> {
    let size = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|length| {
            length
                .to_str()
                .ok()?
                .parse::<u64>()
                .ok()
        })
        .ok_or_else(|| bad_request("a staged archive upload needs a Content-Length".to_string()))?;
    if size > MAX_TARBALL_BYTES {
        return Err(bad_request(format!(
            "the archive is {size} bytes, over the {MAX_TARBALL_BYTES}-byte limit of one archive",
        )));
    }
    Ok(size)
}

/// Reserve `size` bytes for `key`'s archive, unless that takes its principal
/// past its limits. `Ok(false)` when a concurrent upload of the same bytes
/// staged the archive meanwhile, so this upload only renews it.
async fn reserve_within_limits(
    storage: &Storage,
    key: &StagedArchiveKey<'_>,
    size: u64,
) -> Result<bool, RegistryError> {
    // A first, cheap refusal of a principal far over its limit, counting the
    // archives it holds. It allows twice the limit: the archive's own
    // record, renewed or reserved again, is no new one, and the check below
    // is the exact one.
    let listed = storage.staged_archive_count(key.principal).await?;
    if listed >= 2 * MAX_STAGED_ARCHIVES_PER_PRINCIPAL {
        return Err(over_limit(&format!("{MAX_STAGED_ARCHIVES_PER_PRINCIPAL} archives")));
    }
    if !storage.reserve_staged_archive(key, size).await? {
        return Ok(false);
    }
    let usage = storage.staged_archive_usage(key.principal).await?;
    let refusal = if usage.bytes > MAX_STAGED_BYTES_PER_PRINCIPAL {
        over_limit(&format!("{MAX_STAGED_BYTES_PER_PRINCIPAL} bytes"))
    } else if usage.archives > MAX_STAGED_ARCHIVES_PER_PRINCIPAL {
        over_limit(&format!("{MAX_STAGED_ARCHIVES_PER_PRINCIPAL} archives"))
    } else {
        return Ok(true);
    };
    storage.release_staged_archive(key).await?;
    Err(refusal)
}

fn over_limit(limit: &str) -> RegistryError {
    bad_request(format!(
        "staging this archive would hold more than {limit} that no publish has claimed yet; \
         publish or wait for staged archives to expire",
    ))
}

/// Read `body` whole, refusing it unless it is `size` bytes matching
/// `integrity`.
async fn receive_archive(body: Body, integrity: &str, size: u64) -> Result<Vec<u8>, RegistryError> {
    let mut stream = body.into_data_stream();
    // The declared size is the client's word, so the buffer grows with what
    // arrives rather than reserving it up front.
    let mut bytes =
        Vec::with_capacity(usize::try_from(size.min(INITIAL_BUFFER_BYTES)).unwrap_or_default());
    let mut hasher = Sha512::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|err| bad_request(format!("the archive upload was interrupted: {err}")))?;
        if (bytes.len() + chunk.len()) as u64 > size {
            return Err(bad_request(format!(
                "the archive is larger than its declared {size} bytes",
            )));
        }
        hasher.update(&chunk);
        // Past the first buffer the client is sending what it declared, so
        // take the rest in one allocation rather than doubling into it.
        if bytes.len() as u64 >= INITIAL_BUFFER_BYTES
            && bytes.capacity() < bytes.len() + chunk.len()
        {
            bytes.reserve_exact(usize::try_from(size).unwrap_or_default() - bytes.len());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() as u64 != size {
        return Err(bad_request(format!(
            "the archive has {} bytes but declares {size}",
            bytes.len(),
        )));
    }
    verify_blob_digest(integrity, &hasher.finalize())
        .map_err(|err| RegistryError::BadRequest { reason: format!("EINTEGRITY: {err}") })?;
    Ok(bytes)
}

/// The bytes of the archive `claimant` staged by `integrity`, which the
/// attachment `filename` of a publish references.
pub(super) async fn claim_staged_archive(
    storage: &Storage,
    claimant: Option<&str>,
    filename: &str,
    integrity: &str,
) -> Result<Vec<u8>, RegistryError> {
    let not_staged = || {
        bad_request(format!(
            "{filename} references the staged archive {integrity}, which the publishing account \
             has not staged or which has expired; upload it again with \
             PUT /-/pnpr/v0/artifacts/blob?integrity=<integrity> before publishing",
        ))
    };
    let principal = claimant.ok_or_else(not_staged)?;
    let digest = archive_digest(integrity)?;
    storage
        .read_staged_archive(&StagedArchiveKey { principal, digest: &digest })
        .await?
        .ok_or_else(not_staged)
}

/// The staged archives one publish request references, checked before any
/// is read and given back once the request commits.
#[derive(Default)]
pub(super) struct StagedReferences {
    principal: Option<String>,
    digests: Vec<String>,
}

impl StagedReferences {
    /// Check the staged archives `integrities` name, one per referencing
    /// attachment, against what one request may pull in. Each reference is
    /// read, written and decoded on its own, so a request naming one archive
    /// many times costs as much as naming that many archives, and counts so.
    pub(super) async fn check<'a>(
        storage: &Storage,
        identity: &Identity,
        integrities: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, RegistryError> {
        let integrities: Vec<&str> = integrities.into_iter().collect();
        if integrities.is_empty() {
            return Ok(Self::default());
        }
        Self::check_within(
            storage,
            principal(identity)?,
            integrities,
            MAX_STAGED_BYTES_PER_PRINCIPAL,
        )
        .await
    }

    /// [`Self::check`] for `principal`, with `limit` bytes per request.
    async fn check_within(
        storage: &Storage,
        principal: &str,
        integrities: Vec<&str>,
        limit: u64,
    ) -> Result<Self, RegistryError> {
        if integrities.len() > MAX_STAGED_ARCHIVES_PER_PRINCIPAL {
            return Err(bad_request(format!(
                "the publish references {} staged archives, more than the \
                 {MAX_STAGED_ARCHIVES_PER_PRINCIPAL} an account can hold",
                integrities.len(),
            )));
        }
        // Each distinct archive is looked up once, and counted once per
        // reference.
        let mut references: HashMap<String, u64> = HashMap::new();
        for integrity in integrities {
            *references
                .entry(archive_digest(integrity)?)
                .or_default() += 1;
        }
        let mut total = 0_u64;
        for (digest, count) in &references {
            let key = StagedArchiveKey { principal, digest };
            // One that is missing is reported, with its attachment, when it
            // is claimed.
            let size = storage.claimable_staged_archive_size(&key).await?.unwrap_or(0);
            total = total.saturating_add(size.saturating_mul(*count));
        }
        let digests = references.into_keys().collect();
        if total > limit {
            return Err(bad_request(format!(
                "the publish references {total} bytes of staged archives, more than the {limit} \
                 one publish may take",
            )));
        }
        Ok(Self { principal: Some(principal.to_string()), digests })
    }

    /// Retire the archives the committed request claimed, so they stop
    /// counting against their principal. A failure only warns: the archive
    /// expires anyway.
    pub(super) async fn release(self, storage: &Storage) {
        let Some(principal) = self.principal.as_deref() else { return };
        for digest in &self.digests {
            let key = StagedArchiveKey { principal, digest };
            if let Err(error) = storage.retire_staged_archive(&key).await {
                tracing::warn!(%error, "a claimed staged archive was not retired");
            }
        }
    }
}

/// The principal a staged archive belongs to: the caller's stable identity,
/// the one an idempotent publish is scoped to.
pub(super) fn principal(identity: &Identity) -> Result<&str, RegistryError> {
    identity
        .username()
        .ok_or_else(|| RegistryError::Unauthenticated {
            resource: "release archive staging".to_string(),
        })
}

/// The hex SHA-512 digest a staged archive with `integrity` is stored under.
pub(super) fn archive_digest(integrity: &str) -> Result<String, RegistryError> {
    blob_id(integrity).map_err(|err| bad_request(err.to_string()))
}

/// Whether a blob request names a shared artifact's owner.
fn names_owner(uri: &Uri) -> bool {
    query_pairs(uri).any(|(key, _)| matches!(key.as_ref(), "organization" | "publisher"))
}

fn query_integrity(uri: &Uri) -> Result<String, RegistryError> {
    let mut integrities = query_pairs(uri)
        .filter(|(key, _)| key == "integrity")
        .map(|(_, value)| value.into_owned());
    match (integrities.next(), integrities.next()) {
        (Some(integrity), None) => Ok(integrity),
        _ => Err(bad_request("a staged archive request names one integrity".to_string())),
    }
}

fn query_digest(uri: &Uri) -> Result<String, RegistryError> {
    archive_digest(&query_integrity(uri)?)
}

fn query_pairs(uri: &Uri) -> url::form_urlencoded::Parse<'_> {
    url::form_urlencoded::parse(
        uri.query()
            .unwrap_or_default()
            .as_bytes(),
    )
}

fn bad_request(reason: String) -> RegistryError {
    RegistryError::BadRequest { reason }
}

#[cfg(test)]
mod tests;
