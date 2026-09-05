//! The non-npm registry surfaces.
//!
//! A `/~<name>/` request whose registry speaks Cargo or Python is handed here
//! by the npm segment handlers before any npm-shaped reading of the path, so
//! an npm registry keeps every URL it served before and a Cargo or Python
//! registry never has its documents read as packuments. This module holds
//! the dispatch and what the two surfaces share: the registry endpoint URL
//! they write into their own metadata, the anonymous-readability check, and
//! the proxy cache for an upstream's metadata documents and artifacts.

use super::{
    AppState, MAX_TARBALL_BYTES, cached_upstream_tarball, cargo, not_found, pypi, tarball_response,
    tarball_stream_error,
};
use axum::{
    body::Bytes,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::PackageName;
use pnpr_policy::Identity;
use pnpr_registry::{Ecosystem, Registry};
use pnpr_storage::streaming;
use pnpr_upstream::{FetchOutcome, FetchedDocument, Upstream};
use sha2::{Digest, Sha256};
use ssri::{Algorithm, Integrity};

/// The ecosystem `registry` speaks when it is not npm, so a segment handler
/// knows to hand the request to that surface instead of reading it as npm.
pub(super) fn non_npm_ecosystem(state: &AppState, registry: &str) -> Option<Ecosystem> {
    match state.inner.config.registries.ecosystem(registry)? {
        Ecosystem::Npm => None,
        ecosystem @ (Ecosystem::Cargo | Ecosystem::Pypi) => Some(ecosystem),
    }
}

/// Serve a `GET /~<registry>/<segments...>` on a non-npm registry.
pub(super) async fn serve_get(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: &str,
    ecosystem: Ecosystem,
    segments: &[&str],
) -> Response {
    match ecosystem {
        Ecosystem::Cargo => cargo::serve_get(state, identity, registry, segments).await,
        Ecosystem::Pypi => pypi::serve_get(state, identity, headers, registry, segments).await,
        Ecosystem::Npm => not_found(),
    }
}

/// Serve a `PUT /~<registry>/<segments...>` on a non-npm registry.
pub(super) async fn serve_put(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    ecosystem: Ecosystem,
    segments: &[&str],
    body: Bytes,
) -> Response {
    match ecosystem {
        Ecosystem::Cargo => cargo::serve_put(state, identity, registry, segments, body).await,
        Ecosystem::Pypi | Ecosystem::Npm => not_found(),
    }
}

/// Serve a `DELETE /~<registry>/<segments...>` on a non-npm registry.
pub(super) async fn serve_delete(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    ecosystem: Ecosystem,
    segments: &[&str],
) -> Response {
    match ecosystem {
        Ecosystem::Cargo => cargo::serve_delete(state, identity, registry, segments).await,
        Ecosystem::Pypi | Ecosystem::Npm => not_found(),
    }
}

/// The URL clients reach `registry` at, for the URLs a surface writes into
/// the metadata it serves (a Cargo `config.json`, a Simple API page).
pub(super) fn registry_endpoint(state: &AppState, registry: &str) -> String {
    format!("{}/~{registry}", state.inner.config.public_url.trim_end_matches('/'))
}

/// Whether some read through `registry` is closed to anonymous callers: a
/// hosted source whose registry-level default denies them, or an upstream
/// source with an `access:` gate. Drives the Cargo `auth-required` flag,
/// which makes `cargo` send its token on index and download requests too.
pub(super) fn registry_requires_auth(state: &AppState, registry: &str) -> bool {
    let config = &state.inner.config;
    match config.registries.get(registry) {
        Some(Registry::Hosted { .. }) => config
            .hosted
            .get(registry)
            .is_some_and(|hosted| !hosted.rules.default_access().allows(&Identity::Anonymous)),
        Some(Registry::Upstream { .. }) => {
            config.upstreams.get(registry).is_some_and(|upstream| upstream.access.is_some())
        }
        Some(Registry::Router { sources }) => {
            sources.iter().any(|source| registry_requires_auth(state, source))
        }
        None => false,
    }
}

/// The hosted registries a request through `registry` can land on.
pub(super) fn hosted_sources(state: &AppState, registry: &str) -> Vec<String> {
    let registries = &state.inner.config.registries;
    match registries.get(registry) {
        Some(Registry::Hosted { .. }) => vec![registry.to_string()],
        Some(Registry::Router { sources }) => sources
            .iter()
            .filter(|source| matches!(registries.get(source), Some(Registry::Hosted { .. })))
            .cloned()
            .collect(),
        Some(Registry::Upstream { .. }) | None => Vec::new(),
    }
}

/// An upstream metadata document to read through the proxy cache.
pub(super) struct UpstreamDocument<'request> {
    /// The cache key.
    pub(super) name: &'request PackageName,
    /// The path relative to the upstream's base URL.
    pub(super) relative_path: &'request str,
    pub(super) accept: Option<&'request str>,
    pub(super) limit: usize,
}

/// A fresh cached copy of an upstream document, else the document fetched
/// and cached — `encode` turns the fetched response into the cached bytes,
/// so a surface can keep the response URL beside the body. A definitive
/// upstream 404 purges the cache entry. On an upstream failure a stale
/// cached copy is served instead, so a transient outage does not break
/// resolution of what was already known.
pub(super) async fn load_upstream_document(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    request: UpstreamDocument<'_>,
    encode: impl FnOnce(FetchedDocument) -> Result<Vec<u8>, RegistryError>,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let storage = &state.inner.storage;
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    if let Some(bytes) = storage.read_upstream_packument(namespace, request.name, ttl).await? {
        return Ok(Some(bytes));
    }
    let fetched = upstream.fetch_document(request.relative_path, request.accept, request.limit);
    match fetched.await.and_then(|outcome| match outcome {
        FetchOutcome::Ok(document) => encode(document).map(Some),
        FetchOutcome::NotFound => Ok(None),
    }) {
        Ok(Some(bytes)) => {
            storage.write_upstream_packument(namespace, request.name, &bytes).await?;
            Ok(Some(bytes))
        }
        Ok(None) => {
            storage.remove_upstream_package(namespace, request.name).await?;
            Ok(None)
        }
        Err(err) => match storage.read_upstream_packument_any(namespace, request.name).await? {
            Some(stale) => {
                tracing::warn!(
                    ?err,
                    package = %request.name.as_str(),
                    "upstream document refresh failed; serving the stale cached copy",
                );
                Ok(Some(stale))
            }
            None => Err(err),
        },
    }
}

/// Serve an upstream artifact: the cached copy when there is one, else the
/// bytes streamed from `url` into the cache while verified against
/// `integrity` (see `streaming::stream_verified_to_cache` for what a
/// mismatch can and cannot do). A `cache: false` upstream streams through a
/// verified temp file instead.
pub(super) async fn serve_upstream_artifact(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    name: &PackageName,
    filename: &str,
    url: &str,
    integrity: &Integrity,
) -> Response {
    if upstream.caches()
        && let Some(response) = cached_upstream_tarball(state, namespace, name, filename).await
    {
        return response;
    }
    let response = match upstream.fetch_artifact_response(url).await {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let write = match state.inner.storage.open_upstream_tarball_tmp(namespace, name, filename).await
    {
        Ok(write) => write,
        Err(err) => return err.into_response(),
    };
    if !upstream.caches() {
        return match streaming::download_verified_to_temp(
            response,
            write,
            integrity,
            MAX_TARBALL_BYTES,
        )
        .await
        {
            Ok((file, len, tmp_path)) => {
                tarball_response(streaming::stream_file_and_remove(file, tmp_path), Some(len))
            }
            Err(err) => tarball_stream_error(err, name, filename).into_response(),
        };
    }
    match streaming::stream_verified_to_cache(response, write, integrity, MAX_TARBALL_BYTES) {
        Ok(body) => tarball_response(body, None),
        Err(err) => tarball_stream_error(err, name, filename).into_response(),
    }
}

/// The SRI form of a lowercase hex SHA-256, or `None` when it is not one.
pub(super) fn sha256_integrity(hex: &str) -> Option<Integrity> {
    Integrity::from_hex(hex, Algorithm::Sha256).ok()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Whether an artifact URL published by an upstream may be fetched: HTTP(S)
/// without embedded credentials.
pub(super) fn is_fetchable_artifact_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
}
