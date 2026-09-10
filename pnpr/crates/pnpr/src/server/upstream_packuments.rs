use super::{
    Action, AppState, CacheValidators, CanonicalPackageName, Duration, Ecosystem, HeaderMap,
    HostedGate, Identity, PackumentFetch, RegistryError, RegistrySource, Response, Upstream,
    authorize, authorized_upstream, default_registry_target, hosted_gate, hosted_read_namespace,
    not_found, packument_response, resolve_registry_source, timed, upstream_cache_namespace,
    wants_abbreviated,
};
use axum::response::IntoResponse;

/// Serve a single version's manifest (`GET <base>/<pkg>/<version-or-tag>`)
/// through the registry graph. Resolves the package to its one concrete origin,
/// loads that origin's packument, and extracts the requested version with its
/// `dist.tarball` rewritten onto the same origin's base.
/// The stored packument of whichever source a registry routes this package to.
///
/// An upstream registry's per-package rules gate the read, and the hosted gate
/// answers a denial itself — a not-found mask or an explicit-rule 401/403.
/// Both are the same checks [`serve_registry_packument`](super::package_reads::serve_registry_packument) applies.
pub(super) async fn read_source_packument(
    state: &AppState,
    identity: &Identity,
    resolved_source: &RegistrySource,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    match resolved_source {
        RegistrySource::Upstream(source) => {
            authorize(state, identity, resolved_source, name.as_str(), Action::Access)?;
            load_upstream_packument_for(state, identity, source, name).await
        }
        RegistrySource::Hosted(source) => {
            let org = hosted_read_namespace(state, identity, source, name.as_str())?;
            state.inner.storage.for_hosted(&org).read_hosted_document(name).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    }
}

/// Load an upstream route's packument: a fresh per-registry cache entry when one
/// exists, otherwise a fetch through the registry (with its server-side credential)
/// written back to the same namespace. A registry with `cache: false` neither reads
/// nor writes the cache — it streams everything through, refetching each time.
pub(super) async fn load_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if upstream.caches()
        && let Some(bytes) = timed(
            "packument:cache_read",
            name.as_str(),
            state.inner.storage.read_upstream_document(namespace, name, ttl),
        )
        .await?
    {
        return Ok(Some(bytes));
    }
    let fetched = match timed(
        "packument:upstream_fetch",
        name.as_str(),
        upstream.fetch_packument(name, &CacheValidators::default()),
    )
    .await
    {
        Ok(fetched) => fetched,
        Err(err) => {
            return recover_stale_upstream_packument(state, namespace, upstream, name, err).await;
        }
    };
    cache_upstream_packument(state, namespace, upstream, name, fetched).await
}

pub(super) async fn cache_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    fetched: PackumentFetch,
) -> Result<Option<Vec<u8>>, RegistryError> {
    match fetched {
        PackumentFetch::Modified(fetched) => {
            if upstream.caches()
                && let Err(err) = state
                    .inner
                    .storage
                    .write_upstream_document(namespace, name, &fetched.bytes)
                    .await
            {
                tracing::warn!(?err, package = %name.as_str(), "upstream packument cache write failed");
            }
            Ok(Some(fetched.bytes))
        }
        PackumentFetch::NotFound => {
            // The 404 is authoritative: the package is gone from this origin,
            // so drop its cached entry too. Otherwise the stale copy would
            // outlive every TTL and a later transient outage could resurrect
            // the unpublished package through the stale-if-error fallback.
            if upstream.caches()
                && let Err(err) = state.inner.storage.remove_upstream_package(namespace, name).await
            {
                tracing::warn!(
                    ?err,
                    package = %name.as_str(),
                    "failed to purge cached entry after an upstream 404",
                );
            }
            Ok(None)
        }
        // `load_upstream_packument` sends no conditional validators (the upstream
        // cache refetches stale entries rather than revalidating — see
        // `Store::read_upstream_document`), so a well-behaved upstream never
        // answers 304 here. If one does anyway, "not modified" means the cached
        // body is current, so serve it (fresh or stale) rather than a spurious
        // 404 that a client could cache as "package gone".
        PackumentFetch::NotModified => {
            state.inner.storage.read_upstream_document_any(namespace, name).await
        }
    }
}

/// Serve a stale cache entry only when an upstream fetch failed transiently.
/// Authoritative client errors and cache-disabled upstreams preserve the fetch
/// error, while transport, server, and open-circuit failures may use bytes from
/// the same upstream namespace.
pub(super) async fn recover_stale_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    err: RegistryError,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if !err.is_transient_upstream_error() || !upstream.caches() {
        return Err(err);
    }
    let Some(bytes) = state.inner.storage.read_upstream_document_any(namespace, name).await? else {
        return Err(err);
    };
    // The upstream error may embed credentials in its request URL, so only its
    // credential-redacted rendering is safe to log.
    tracing::warn!(
        error = %err.log_message(),
        package = %name.as_str(),
        "upstream packument refetch failed; serving stale cache",
    );
    Ok(Some(bytes))
}

/// Authorize and load an upstream registry's packument bytes (from its per-registry
/// private cache, else a fresh fetch through the registry), or a [`Response`]
/// error the caller should return. Shared by the packument and version-manifest
/// serving paths.
pub(super) async fn load_upstream_packument_for(
    state: &AppState,
    identity: &Identity,
    upstream: &str,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let namespace = upstream_cache_namespace(state, upstream);
    let upstream = authorized_upstream(state, identity, upstream)?;
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    load_upstream_packument(state, &namespace, upstream, name, ttl).await
}

/// Load a package's packument bytes through the addressed `/~<name>/` (or,
/// path-less, the default-target registry) — resolving to one concrete origin and
/// reading there, with no fall-through. `Ok(None)` is a definitive not-found
/// (unknown package, no route, no default target, or an unauthorized private
/// hosted org). Used by the readers that aren't
/// packument/tarball/version-manifest (e.g. `dist-tags`).
pub(super) async fn load_packument_for_read(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let target = match registry {
        Some(registry) => registry.to_string(),
        None => match default_registry_target(state, Ecosystem::Npm) {
            Some(target) => target,
            None => return Ok(None),
        },
    };
    // The resolved registry's per-package rules apply to every served read,
    // upstream or hosted — otherwise a restricted package would leak (e.g.
    // its dist-tags) through these path-less readers. A hosted denial is a
    // not-found mask rather than a 401/403 that reveals existence (see
    // `serve_registry_packument`).
    let resolved_source = resolve_registry_source(state, &target, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            authorize(state, identity, &resolved_source, name.as_str(), Action::Access)?;
            load_upstream_packument_for(state, identity, source, name).await
        }
        RegistrySource::Hosted(source) => {
            let org = match hosted_gate(state, identity, source, name.as_str()) {
                HostedGate::Allowed(org) => org,
                HostedGate::MaskNotFound => return Ok(None),
                HostedGate::Denied(err) => return Err(err),
            };
            state.inner.storage.for_hosted(&org).read_hosted_document(name).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    }
}

pub(super) async fn serve_packument_via_upstream(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    upstream: &str,
    name: &CanonicalPackageName,
    tarball_base: &str,
    revision_registry: Option<&str>,
) -> Response {
    let bytes = match load_upstream_packument_for(state, identity, upstream, name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    match packument_response(
        name,
        &bytes,
        tarball_base,
        revision_registry,
        state.inner.osv_index.as_ref(),
        wants_abbreviated(headers),
    ) {
        Ok(response) => response,
        Err(err) => err.into_response(),
    }
}
