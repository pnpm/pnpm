use super::{
    Action, AppState, CacheValidators, CanonicalPackageName, DistView, Duration, Ecosystem,
    HeaderMap, HostedGate, Identity, PackumentFetch, RegistryError, RegistrySource, Response,
    Upstream, authorize, authorized_upstream, default_registry_target, hosted_gate,
    hosted_read_namespace, not_found,
    package_responses::processing::admission::{DocumentSource, SourceAdmission},
    packument_response, resolve_registry_source, timed, upstream_cache_namespace,
};
use axum::{body::Bytes, response::IntoResponse};
use std::sync::Arc;

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
            state.inner.storage
                .for_hosted(&org, pnpr_registry::Ecosystem::Npm)
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
    let document = fetch_coalesced_upstream_packument(state, namespace, upstream, name, ttl).await?;
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
        state.inner.storage.read_upstream_document(namespace, name, ttl),
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
    bytes: Bytes,
    _admission: SourceAdmission,
}

async fn fetch_coalesced_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Arc<AdmittedPackument>>, RegistryError> {
    let key = (namespace.to_string(), name.clone());
    Box::pin(state.inner.proxy.packument_singleflight.work(key, || {
        Box::pin(async {
            let admission =
                state.inner.packuments.admissions.acquire(DocumentSource::Upstream(namespace))
                    .await;
            let fetched =
                fetch_and_cache_upstream_packument(state, namespace, upstream, name, ttl).await?;
            Ok(fetched.map(|bytes| {
                Arc::new(AdmittedPackument { bytes: bytes.into(), _admission: admission })
            }))
        })
    }))
    .await
    .map_err(RegistryError::Shared)
}

async fn fetch_and_cache_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Vec<u8>>, Arc<RegistryError>> {
    if upstream.caches()
        && let Ok(Some(bytes)) =
            state.inner.storage.read_upstream_document(namespace, name, ttl).await
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
            return recover_stale_upstream_packument(state, namespace, upstream, name, err)
                .await
                .map_err(Arc::new);
        }
    };
    cache_upstream_packument(state, namespace, upstream, name, fetched).await.map_err(Arc::new)
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
            // Pins are published into the cached copy, so a packument that
            // failed to cache is served as fetched.
            if upstream.caches()
                && write_cached_packument(state, namespace, name, &fetched.bytes).await
            {
                return super::upstream_integrity::complete_missing_tarball_integrities(
                    state,
                    namespace,
                    upstream,
                    name,
                    fetched.bytes,
                )
                .await
                .map(Some);
            }
            Ok(Some(fetched.bytes))
        }
        PackumentFetch::NotFound => {
            // The 404 is authoritative: the package is gone from this origin,
            // so drop its cached entry too. Otherwise the stale copy would
            // outlive every TTL and a later transient outage could resurrect
            // the unpublished package through the stale-if-error fallback.
            if upstream.caches() {
                purge_cached_package(state, namespace, name).await;
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

async fn write_cached_packument(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    bytes: &[u8],
) -> bool {
    let _guard = lock_upstream_package(state, namespace, name).await;
    let written = state.inner.storage.write_upstream_document(namespace, name, bytes).await;
    if let Err(err) = &written {
        tracing::warn!(?err, package = %name.as_str(), "upstream packument cache write failed");
    }
    written.is_ok()
}

async fn purge_cached_package(state: &AppState, namespace: &str, name: &CanonicalPackageName) {
    let _guard = lock_upstream_package(state, namespace, name).await;
    if let Err(err) = state.inner.storage.remove_upstream_package(namespace, name).await {
        tracing::warn!(
            ?err,
            package = %name.as_str(),
            "failed to purge cached entry after an upstream 404",
        );
    }
}

/// Serializes the writes to one upstream package's cached packument: a refresh,
/// a purge, and publishing computed integrities. The lock is striped and shared
/// with hosted publishes, so no network I/O may run under it.
pub(super) async fn lock_upstream_package<'a>(
    state: &'a AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) -> tokio::sync::MutexGuard<'a, ()> {
    let lock_key = format!("upstream:{namespace}:{}", name.as_str());
    state.inner.locks.packages.lock(&lock_key).await
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
    let ttl = upstream.maxage().unwrap_or(state.inner.config.http.packument_ttl);
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
            state.inner.storage
                .for_hosted(&org, pnpr_registry::Ecosystem::Npm)
                .read_hosted_document(name)
                .await
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
    view: DistView<'_>,
) -> Response {
    let client = match authorized_upstream(state, identity, upstream) {
        Ok(client) => client,
        Err(err) => return err.into_response(),
    };
    let document = UpstreamDocument {
        state,
        upstream: client,
        namespace: upstream_cache_namespace(state, upstream),
        name,
        ttl: client.maxage().unwrap_or(state.inner.config.http.packument_ttl),
        response: DocumentResponse { headers, tarball_base, view },
    };
    match document.cached().await {
        Ok(Some(response)) => response,
        Ok(None) => document.fetched().await,
        Err(err) => err.into_response(),
    }
}

struct UpstreamDocument<'a> {
    state: &'a AppState,
    upstream: &'a Upstream,
    namespace: String,
    name: &'a CanonicalPackageName,
    ttl: Duration,
    response: DocumentResponse<'a>,
}

struct DocumentResponse<'a> {
    headers: &'a HeaderMap,
    tarball_base: &'a str,
    view: DistView<'a>,
}

impl UpstreamDocument<'_> {
    async fn cached(&self) -> Result<Option<Response>, RegistryError> {
        if !self.upstream.caches() {
            return Ok(None);
        }
        let response = async {
            let Some(bytes) =
                read_cached_upstream_packument(self.state, &self.namespace, self.name, self.ttl)
                    .await?
            else {
                return Ok(None);
            };
            self.render(bytes.into()).await.map(Some)
        };
        self.state.inner.packuments.admissions.serve(DocumentSource::Stored, response).await
    }

    async fn fetched(&self) -> Response {
        let document = match fetch_coalesced_upstream_packument(
            self.state,
            &self.namespace,
            self.upstream,
            self.name,
            self.ttl,
        )
        .await
        {
            Ok(Some(document)) => document,
            Ok(None) => return not_found(),
            Err(err) => return err.into_response(),
        };
        let rendering = self.render(document.bytes.clone());
        let response =
            self.state.inner.packuments.admissions.serve(DocumentSource::Stored, rendering).await;
        drop(document);
        match response {
            Ok(response) => response,
            Err(err) => err.into_response(),
        }
    }

    async fn render(&self, bytes: Bytes) -> Result<Response, RegistryError> {
        let table = self.state.inner.storage.read_tar_integrities(self.name).await?;
        let view = DistView { tar_integrities: Some(&table), ..self.response.view };
        packument_response(
            self.name,
            bytes,
            self.response.tarball_base,
            view,
            self.state,
            self.response.headers,
            self.ttl.as_secs(),
        )
        .await
    }
}
