//! What the non-npm registry surfaces share.
//!
//! Every ecosystem is served under its own URL prefix — `/npm/`, `/cargo/`,
//! `/pypi/` — where `/<ecosystem>/~<name>/...` addresses a registry and
//! `/<ecosystem>/...` the default target. The prefix selects the protocol and
//! the registry graph is consulted for that ecosystem only, so one router can
//! front every ecosystem. This module holds the pieces the Cargo and Python
//! surfaces both need: addressing (which registry, which endpoint URL, which
//! cache headers), the anonymous-readability check, and the proxy cache for
//! an upstream's metadata documents and artifacts.

use super::{
    AppState, MAX_TARBALL_BYTES, cached_upstream_tarball, default_registry_target, not_found,
    private_no_cache, resolves_to_private_source, tarball_response, tarball_stream_error,
};
use axum::response::{IntoResponse, Response};
use pnpr_error::RegistryError;
use pnpr_package_name::PackageName;
use pnpr_policy::Identity;
use pnpr_registry::{Ecosystem, Registry};
use pnpr_storage::streaming;
use pnpr_upstream::{FetchOutcome, FetchedDocument, Upstream};
use sha2::{Digest, Sha256};
use ssri::{Algorithm, Integrity};

/// The registry a request addressed: the `~<name>` it named, else the
/// configured default target. `None` when the path-less form has no default.
pub(super) fn addressed_registry(state: &AppState, registry: Option<&str>) -> Option<String> {
    registry.map(str::to_string).or_else(|| default_registry_target(state))
}

/// The URL clients reach the addressed registry at for `ecosystem`, for the
/// URLs a surface writes into the metadata it serves (a Cargo `config.json`,
/// a Simple API page): `<public_url>/<ecosystem>` for the default target,
/// `<public_url>/<ecosystem>/~<name>` for a named registry.
pub(super) fn registry_endpoint(
    state: &AppState,
    ecosystem: Ecosystem,
    registry: Option<&str>,
) -> String {
    let base = format!("{}/{ecosystem}", state.inner.config.public_url.trim_end_matches('/'));
    match registry {
        Some(registry) => format!("{base}/~{registry}"),
        None => base,
    }
}

/// The cache headers a response on an ecosystem surface needs: a named
/// registry's responses are always caller-scoped (like npm's `/~<name>/`
/// surface), the default target's only when `package` resolves to a
/// caller-gated source, so the hot public install path stays cacheable.
pub(super) fn caller_scoped(
    state: &AppState,
    ecosystem: Ecosystem,
    registry: Option<&str>,
    package: Option<&str>,
    response: Response,
) -> Response {
    if registry.is_some() {
        return private_no_cache(response);
    }
    match (default_registry_target(state), package) {
        (Some(target), Some(package))
            if resolves_to_private_source(state, &target, ecosystem, package) =>
        {
            private_no_cache(response)
        }
        _ => response,
    }
}

/// Whether some read of `ecosystem` through `registry` is closed to anonymous
/// callers: a hosted source whose registry-level default denies them, or an
/// upstream source with an `access:` gate. Drives the Cargo `auth-required`
/// flag, which makes `cargo` send its token on index and download requests too.
pub(super) fn registry_requires_auth(
    state: &AppState,
    registry: &str,
    ecosystem: Ecosystem,
) -> bool {
    let config = &state.inner.config;
    config.registries.sources(registry, ecosystem).into_iter().any(|source| {
        match config.registries.get(source) {
            Some(Registry::Hosted { .. }) => config
                .hosted
                .get(source)
                .is_some_and(|hosted| !hosted.rules.default_access().allows(&Identity::Anonymous)),
            Some(Registry::Upstream { .. }) => {
                config.upstreams.get(source).is_some_and(|upstream| upstream.access.is_some())
            }
            Some(Registry::Router { .. }) | None => false,
        }
    })
}

/// The hosted registries of `ecosystem` a request through `registry` can land on.
pub(super) fn hosted_sources(
    state: &AppState,
    registry: &str,
    ecosystem: Ecosystem,
) -> Vec<String> {
    let registries = &state.inner.config.registries;
    registries
        .sources(registry, ecosystem)
        .into_iter()
        .filter(|source| matches!(registries.get(source), Some(Registry::Hosted { .. })))
        .map(str::to_string)
        .collect()
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
