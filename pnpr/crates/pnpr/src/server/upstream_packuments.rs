pub(in crate::server) mod purge;

pub(super) use missing_versions::{
    PackumentRefetches,
    REFETCH_INTERVAL,
    reload_source_packument_lacking_version,
    reload_upstream_packument_for_expired_tarball,
    reload_upstream_packument_lacking_version,
};
pub(super) use purge::cache_upstream_packument;
pub(super) use serving::serve_packument_via_upstream;
pub(super) use write_order::{
    PackumentWriteOrder,
    PurgeRemoval,
    WriteVerdict,
};

use super::{
    Action,
    AppState,
    CacheValidators,
    CanonicalPackageName,
    DistView,
    Duration,
    Ecosystem,
    HeaderMap,
    HostedGate,
    Identity,
    PackumentFetch,
    RegistryError,
    RegistrySource,
    Response,
    Upstream,
    authorize,
    authorized_upstream,
    default_registry_target,
    hosted_gate,
    hosted_read_namespace,
    not_found,
    package_responses::processing::admission::{
        DocumentSource,
        SourceAdmission,
    },
    packument_response,
    resolve_registry_source,
    timed,
    upstream_cache_namespace,
};
use axum::body::Bytes;
use purge::{
    read_unpurged_upstream_document,
    read_unpurged_upstream_document_any,
};
use std::{
    future::Future,
    sync::Arc,
};

mod missing_versions;
mod serving;
mod write_order;

/// The stored packument of whichever source a registry routes this package to.
///
/// An upstream registry's per-package rules gate the read, and the hosted gate
/// answers a denial itself, a not-found mask or an explicit-rule 401/403.
/// Both are the same checks [`serve_registry_packument`](super::package_reads::serve_registry_packument) applies.
pub(super) async fn read_source_packument(
    state: &AppState,
    identity: &Identity,
    resolved_source: &RegistrySource,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    match resolved_source {
        RegistrySource::Upstream(source) => {
            authorize(state, identity, resolved_source, name.as_str(), Action::Read)?;
            load_upstream_packument_for(state, identity, source, name).await
        }
        RegistrySource::Hosted(source) => {
            let namespace = hosted_read_namespace(state, identity, source, name.as_str())?;
            state.inner.storage
                .for_hosted(&namespace, pnpr_registry::Ecosystem::Npm)
                .read_hosted_document(name)
                .await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    }
}

/// Load an upstream route's packument: a fresh per-registry cache entry when one
/// exists, otherwise a coalesced fetch through the registry (with its server-side credential)
/// written back to the same namespace. Concurrent requests for the same package/namespace
/// are coalesced so concurrent cache misses do not hammer the upstream registry.
pub(super) async fn load_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if upstream.caches() {
        let cached = read_cached_upstream_packument(state, namespace, name, ttl);
        if let Some(bytes) =
            state.inner.packuments.admissions.serve(DocumentSource::Stored, cached).await?
        {
            return Ok(Some(bytes));
        }
    }
    let document =
        Box::pin(fetch_coalesced_upstream_packument(state, namespace, upstream, name, ttl)).await?;
    Ok(document.map(|document| document.bytes.to_vec()))
}

async fn read_cached_upstream_packument(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let bytes = timed(
        "packument:cache_read",
        name.as_str(),
        read_unpurged_upstream_document(state, namespace, name, ttl),
    )
    .await?;
    if bytes.is_some() {
        state.inner.metrics.record_cache_hit();
    } else {
        state.inner.metrics.record_cache_miss();
    }
    Ok(bytes)
}

pub(super) struct AdmittedPackument {
    pub(super) bytes: Bytes,
    _admission: SourceAdmission,
}

impl AdmittedPackument {
    fn new(bytes: Vec<u8>, admission: SourceAdmission) -> Arc<Self> {
        Arc::new(Self { bytes: bytes.into(), _admission: admission })
    }
}

pub(super) async fn fetch_coalesced_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Arc<AdmittedPackument>>, RegistryError> {
    coalesce_upstream_packument(state, namespace, name, || async {
        let admission =
            state.inner.packuments.admissions.acquire(DocumentSource::Upstream(namespace)).await;
        let fetched =
            fetch_and_cache_upstream_packument(state, namespace, upstream, name, Some(ttl)).await?;
        Ok(fetched.map(|bytes| AdmittedPackument::new(bytes, admission)))
    })
    .await
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PackumentFlightKind {
    Read,
    ForcedRefresh,
}

/// Run `read` as the one in-flight read of an upstream package's packument,
/// or wait for the read already in flight for it.
async fn coalesce_upstream_packument<Read, Pending>(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    read: Read,
) -> Result<Option<Arc<AdmittedPackument>>, RegistryError>
where
    Read: FnOnce() -> Pending + Send,
    Pending: Future<Output = Result<Option<Arc<AdmittedPackument>>, Arc<RegistryError>>> + Send,
{
    coalesce_upstream_packument_keyed(
        state,
        (namespace.to_string(), name.clone(), PackumentFlightKind::Read),
        read,
    )
    .await
}

async fn coalesce_upstream_packument_keyed<Read, Pending>(
    state: &AppState,
    key: (String, CanonicalPackageName, PackumentFlightKind),
    read: Read,
) -> Result<Option<Arc<AdmittedPackument>>, RegistryError>
where
    Read: FnOnce() -> Pending + Send,
    Pending: Future<Output = Result<Option<Arc<AdmittedPackument>>, Arc<RegistryError>>> + Send,
{
    Box::pin(state.inner.proxy.packument_singleflight.work(key, || Box::pin(read())))
        .await
        .map_err(RegistryError::Shared)
}

async fn fetch_and_cache_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Option<Duration>,
) -> Result<Option<Vec<u8>>, Arc<RegistryError>> {
    if let Some(ttl) = ttl
        && upstream.caches()
        && let Ok(Some(bytes)) = read_unpurged_upstream_document(state, namespace, name, ttl).await
    {
        return Ok(Some(bytes));
    }

    let fetch_seq = state.inner.proxy.packument_writes.begin_fetch();
    let fetched = match timed(
        "packument:upstream_fetch",
        name.as_str(),
        upstream.fetch_packument(name, &CacheValidators::default()),
    )
    .await
    {
        Ok(fetched) => fetched,
        Err(err) => {
            state.inner.proxy.packument_refetches.record_failure(namespace.to_owned(), name);
            return recover_stale_upstream_packument(state, namespace, upstream, name, err)
                .await
                .map_err(Arc::new);
        }
    };
    cache_upstream_packument(state, namespace, upstream, name, fetched, fetch_seq)
        .await
        .map_err(Arc::new)
}

/// Serializes the writes to one upstream package's cached packument: a refresh,
/// a purge, and publishing computed integrities. The lock is striped and shared
/// with hosted publishes, so no network I/O may run under it.
pub(super) async fn lock_upstream_package<'a>(
    state: &'a AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) -> tokio::sync::MutexGuard<'a, ()> {
    let lock_key = upstream_package_key(namespace, name);
    state.inner.locks.packages.lock(&lock_key).await
}

pub(super) fn upstream_package_generation(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) -> u64 {
    state.inner.locks.packages.generation(&upstream_package_key(namespace, name))
}

pub(super) fn invalidate_upstream_package(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) {
    state.inner.locks.packages.advance_generation(&upstream_package_key(namespace, name));
}

fn upstream_package_key(namespace: &str, name: &CanonicalPackageName) -> String {
    format!("upstream:{namespace}:{}", name.as_str())
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
    let Some(bytes) = read_unpurged_upstream_document_any(state, namespace, name).await? else {
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
    let ttl = upstream.packument_ttl().unwrap_or(state.inner.config.http.packument_ttl);
    load_upstream_packument(state, &namespace, &upstream, name, ttl).await
}

/// Load a package's packument bytes through the addressed `/~<name>/` (or,
/// path-less, the default-target registry), resolving to one concrete origin and
/// reading there, with no fall-through. `Ok(None)` is a definitive not-found
/// (unknown package, no route, no default target, or an unauthorized private
/// hosted registry). Used by the readers that aren't
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
    // upstream or hosted; otherwise a restricted package would leak (e.g.
    // its dist-tags) through these path-less readers. A hosted denial is a
    // not-found mask rather than a 401/403 that reveals existence (see
    // `serve_registry_packument`).
    let resolved_source = resolve_registry_source(state, &target, name.as_str());
    match &resolved_source {
        RegistrySource::Upstream(source) => {
            authorize(state, identity, &resolved_source, name.as_str(), Action::Read)?;
            load_upstream_packument_for(state, identity, source, name).await
        }
        RegistrySource::Hosted(source) => {
            let namespace = match hosted_gate(state, identity, source, name.as_str()) {
                HostedGate::Allowed(namespace) => namespace,
                HostedGate::MaskNotFound => return Ok(None),
                HostedGate::Denied(err) => return Err(err),
            };
            state.inner.storage
                .for_hosted(&namespace, pnpr_registry::Ecosystem::Npm)
                .read_hosted_document(name)
                .await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    }
}
