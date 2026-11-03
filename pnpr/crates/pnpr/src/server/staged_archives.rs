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
use pnpr_storage::StagedArchiveKey;
use sha2::{Digest as _, Sha512};

/// The most a principal may hold staged at once, across all its archives.
/// Archives of a release wait here only until its commit, so this is many
/// releases' worth.
pub(super) const MAX_STAGED_BYTES_PER_PRINCIPAL: u64 = 2 * 1024 * 1024 * 1024;

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
        if state.inner.builds.artifacts.is_none() {
            return private_no_cache(StatusCode::NOT_FOUND.into_response());
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
) -> Response {
    if names_owner(&uri) {
        if state.inner.builds.artifacts.is_none() {
            return private_no_cache(StatusCode::NOT_FOUND.into_response());
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
    let storage = &state.inner.storage;
    let key = StagedArchiveKey { principal, digest: &digest };
    let renewing = storage.staged_archive_size(&key).await?.is_some();
    if !renewing {
        let usage = storage.staged_archive_usage(principal).await?;
        if usage.saturating_add(size) > MAX_STAGED_BYTES_PER_PRINCIPAL {
            return Err(bad_request(format!(
                "staging this archive would hold more than {MAX_STAGED_BYTES_PER_PRINCIPAL} bytes \
                 of archives no publish has claimed yet; publish or wait for staged archives to \
                 expire",
            )));
        }
    }
    let bytes = receive_archive(body, &integrity, size).await?;
    storage.stage_archive(&key, bytes).await
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
