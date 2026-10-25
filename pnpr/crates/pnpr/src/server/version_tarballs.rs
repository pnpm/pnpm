//! The `<name>@<version>` tarball route every registry serves: the published
//! `.tgz`, and on a tar registry the tar it decodes to.
//!
//! A `.tgz` registry serves a hosted version's blob, or an upstream version's
//! cached `.tgz`, under the canonical filename, so a warm hit loads no
//! packument. Otherwise the route reads the version's dist, which a tar
//! registry also needs for the `.tgz` integrity: from a fresh cached upstream
//! packument, else from the upstream's version manifest, else from the
//! packument. A dist read from an upstream is kept in a [`VersionTgzCache`].

pub(super) use cache::VersionTgzCache;

use cache::{DistOrigin, VersionTgz, version_manifest_tgz, version_tgz};

use super::{
    Action, AppState, CachedTgz, CanonicalPackageName, DistBlock, Ecosystem, FetchOutcome,
    HeaderMap, Identity, PackumentDists, RegistryError, RegistrySource, Response, StatusCode,
    TarSource, TarballOrigin, TgzTrust, Upstream, UpstreamTarball, Value, addressed_registry,
    authorize, authorized_upstream, cached_upstream_tarball, caller_scoped,
    declared_tarball_integrity, ensure_osv_allowed, is_tar_registry, not_found,
    read_source_packument, recover_stale_upstream_packument, resolve_registry_source,
    serve_bound_upstream_tarball, serve_registry_tarball, serve_revision_tarball, tar_decode_limit,
    tar_response, tarball_basename_for_package, tarball_cache_name, tgz_integrity_key, timed,
    upstream_cache_namespace, version_route_tarball_version,
};
use axum::response::IntoResponse;
use pnpr_storage::DocumentStamp;
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

mod cache;

/// The largest upstream version manifest the route reads.
const MAX_VERSION_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

/// `GET {base}/<name>@<version>`: the version's tarball, as the published
/// `.tgz` on a `.tgz` registry, as its tar on a tar registry, and as a
/// revision's tar when `<version>` carries `+r<revision>`.
pub(super) async fn serve_package_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: Option<&str>,
    package: (&str, &str),
) -> Response {
    let (raw_name, version) = package;
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    if let Some(parsed) = super::revision_tarballs::parse_revision_version(version) {
        let response = match parsed {
            Ok((base_version, revision)) => {
                serve_revision_tarball(
                    state,
                    identity,
                    headers,
                    &target,
                    (raw_name, base_version, revision),
                )
                .await
            }
            Err(()) => not_found(),
        };
        return caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response);
    }
    let request = VersionRequest { state, identity, target: &target, raw_name, version };
    let response = request.serve(headers).await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}

struct VersionRequest<'a> {
    state: &'a AppState,
    identity: &'a Identity,
    target: &'a str,
    raw_name: &'a str,
    version: &'a str,
}

impl VersionRequest<'_> {
    async fn serve(&self, headers: &HeaderMap) -> Response {
        let name =
            match CanonicalPackageName::parse(self.raw_name, pnpr_package_name::Ecosystem::Npm) {
                Ok(name) => name,
                Err(err) => return err.into_response(),
            };
        // The version names cache files and upstream URLs.
        if !pnpr_package_name::is_safe_path_segment(self.version) {
            return not_found();
        }
        // Screen the requested version: a hosted manifest's tarball filename
        // may name another version.
        if let Err(err) = ensure_osv_allowed(self.state, &name, self.version) {
            return err.into_response();
        }
        let tar_registry = is_tar_registry(self.state, self.target);
        let source = resolve_registry_source(self.state, self.target, name.as_str());
        if !tar_registry && let Some(response) = self.serve_canonical_tgz(&name, &source).await {
            return response;
        }
        let tgz = match self.version_tgz(&source, &name).await {
            Ok(Some(tgz)) => tgz,
            Ok(None) => return not_found(),
            Err(err) => return err.into_response(),
        };
        let (response, trust) = match &source {
            RegistrySource::Upstream(upstream) => self.serve_upstream(upstream, &name, &tgz).await,
            RegistrySource::Hosted(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => {
                let response = serve_registry_tarball(
                    self.state,
                    self.identity,
                    self.target,
                    self.raw_name,
                    &tgz.filename,
                )
                .await;
                (response, TgzTrust::Verified)
            }
        };
        if !tar_registry || response.status() != StatusCode::OK {
            return response;
        }
        let source = self.tar_source(name, &tgz, &source, trust).await;
        tar_response(self.state, headers, response, source).await
    }

    /// Serve the version's `.tgz` without reading its packument: a hosted
    /// blob, which publish names canonically, or a cached upstream `.tgz`
    /// whose filename [`VersionTgzCache`] already bound to this version.
    /// `None` leaves the version to its packument, including a `dist.tarball`
    /// that names another file.
    async fn serve_canonical_tgz(
        &self,
        name: &CanonicalPackageName,
        source: &RegistrySource,
    ) -> Option<Response> {
        match source {
            RegistrySource::Hosted(_) => {
                let canonical = name.tarball_name_for_version(self.version);
                let response = serve_registry_tarball(
                    self.state,
                    self.identity,
                    self.target,
                    self.raw_name,
                    &canonical,
                )
                .await;
                (response.status() != StatusCode::NOT_FOUND).then_some(response)
            }
            // With OSV screening on, the filename route binds a cached `.tgz`
            // to the version its packument declares for it before serving it.
            RegistrySource::Upstream(_) if self.state.inner.osv_index.is_none() => {
                let cached = match self.cached_packument(source, name) {
                    Ok(cached) => cached?,
                    Err(err) => return Some(err.into_response()),
                };
                let tgz = self.state.inner.proxy.version_tgzs.get_bound(
                    &cached.namespace,
                    name,
                    self.version,
                )?;
                let (filename, _) = tarball_cache_name(name, &tgz.filename).ok()?;
                cached_upstream_tarball(self.state, &cached.namespace, name, &filename).await
            }
            _ => None,
        }
    }

    /// The requested version's dist in `source`, from the [`VersionTgzCache`]
    /// or the upstream.
    async fn version_tgz(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
    ) -> Result<Option<Arc<VersionTgz>>, RegistryError> {
        let cached = self.cached_packument(source, name)?;
        let stamp = match &cached {
            Some(cached) => self.fresh_packument_stamp(cached, name).await?,
            None => None,
        };
        let cache = &self.state.inner.proxy.version_tgzs;
        if let Some(cached) = &cached
            && let Some(tgz) = cache.get(&cached.namespace, name, self.version, stamp, cached.ttl)
        {
            return Ok(Some(tgz));
        }
        let Some((tgz, origin)) =
            self.read_version_tgz(source, name, cached.as_ref(), stamp).await?
        else {
            return Ok(None);
        };
        let tgz = Arc::new(tgz);
        if let (Some(cached), Some(origin)) = (cached, origin) {
            cache.put(cached.namespace, name, self.version, origin, Arc::clone(&tgz));
        }
        Ok(Some(tgz))
    }

    /// Read the version's dist and say where it came from, when it may be
    /// cached. Without a fresh cached packument, an upstream's version
    /// manifest is fetched rather than its whole packument. The packument
    /// answers when the manifest does not, as for an upstream that serves
    /// none.
    async fn read_version_tgz(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
        cached: Option<&CachedPackument>,
        stamp: Option<DocumentStamp>,
    ) -> Result<Option<(VersionTgz, Option<DistOrigin>)>, RegistryError> {
        if stamp.is_none()
            && let RegistrySource::Upstream(id) = source
            && let Some(read) = self.read_version_manifest(id, name, cached).await
        {
            return Ok(Some(read));
        }
        let Some(packument) = read_source_packument(self.state, self.identity, source, name).await?
        else {
            return Ok(None);
        };
        let stamp = match (stamp, cached) {
            (Some(stamp), _) => Some(stamp),
            (None, Some(cached)) => {
                self.state.inner.storage.upstream_document_stamp(&cached.namespace, name).await?
            }
            (None, None) => None,
        };
        Ok(version_tgz(&packument, name, self.version)
            .map(|tgz| (tgz, stamp.map(DistOrigin::Packument))))
    }

    /// The version's dist from its manifest, or from the stale cached
    /// packument when the upstream is unreachable. `None` leaves the version
    /// to its packument.
    async fn read_version_manifest(
        &self,
        id: &str,
        name: &CanonicalPackageName,
        cached: Option<&CachedPackument>,
    ) -> Option<(VersionTgz, Option<DistOrigin>)> {
        let upstream = authorized_upstream(self.state, self.identity, id).ok()?;
        let fetched = Instant::now();
        let err = match self.fetch_version_manifest(upstream, name).await {
            ManifestRead::Dist(tgz) => {
                return Some((tgz, Some(DistOrigin::VersionManifest(fetched))));
            }
            ManifestRead::Unreachable(err) => err,
            ManifestRead::Unusable => return None,
        };
        // A packument fetch would wait on the same upstream again.
        let stale =
            recover_stale_upstream_packument(self.state, &cached?.namespace, upstream, name, err)
                .await
                .ok()??;
        version_tgz(&stale, name, self.version).map(|tgz| (tgz, None))
    }

    /// The version's dist from `<upstream>/<name>/<version>`.
    async fn fetch_version_manifest(
        &self,
        upstream: &Upstream,
        name: &CanonicalPackageName,
    ) -> ManifestRead {
        let path = format!("{}/{}", name.as_str(), self.version);
        let fetched = timed(
            "version_manifest:upstream_fetch",
            name.as_str(),
            upstream.fetch_document(&path, Some("application/json"), MAX_VERSION_MANIFEST_BYTES),
        )
        .await;
        match fetched {
            Ok(FetchOutcome::Ok(manifest)) => {
                version_manifest_tgz(&manifest.bytes, name, self.version)
                    .map_or(ManifestRead::Unusable, ManifestRead::Dist)
            }
            Ok(FetchOutcome::NotFound) => ManifestRead::Unusable,
            Err(
                err @ (RegistryError::Upstream { .. } | RegistryError::UpstreamUnavailable { .. }),
            ) => ManifestRead::Unreachable(err),
            // A status, even a 5xx, may only mean the upstream serves no
            // manifests, so its packument is fetched.
            Err(err) => {
                tracing::debug!(
                    error = %err.log_message(),
                    package = %name.as_str(),
                    "version manifest fetch failed",
                );
                ManifestRead::Unusable
            }
        }
    }

    /// The upstream cache `source` reads `name`'s packument from, once the
    /// caller may read it there. `None` for a hosted registry and for a
    /// `cache: false` upstream.
    fn cached_packument(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
    ) -> Result<Option<CachedPackument>, RegistryError> {
        let RegistrySource::Upstream(id) = source else { return Ok(None) };
        authorize(self.state, self.identity, source, name.as_str(), Action::Access)?;
        let upstream = authorized_upstream(self.state, self.identity, id)?;
        if !upstream.caches() {
            return Ok(None);
        }
        Ok(Some(CachedPackument {
            namespace: upstream_cache_namespace(self.state, id),
            ttl: upstream.maxage().unwrap_or(self.state.inner.config.http.packument_ttl),
        }))
    }

    /// The stamp of the cached packument, while it is fresh enough to serve.
    async fn fresh_packument_stamp(
        &self,
        cached: &CachedPackument,
        name: &CanonicalPackageName,
    ) -> Result<Option<DocumentStamp>, RegistryError> {
        let stamp =
            self.state.inner.storage.upstream_document_stamp(&cached.namespace, name).await?;
        Ok(stamp.filter(|stamp| {
            SystemTime::now().duration_since(stamp.modified).unwrap_or(Duration::ZERO) <= cached.ttl
        }))
    }

    async fn tar_source(
        &self,
        name: CanonicalPackageName,
        tgz: &VersionTgz,
        source: &RegistrySource,
        trust: TgzTrust,
    ) -> TarSource {
        let (tar_integrity_known, derived_unpacked_size) = match &tgz.integrity_key {
            Some(key) => self.state.inner.storage
                .read_tar_integrities(&name)
                .await
                .ok()
                .and_then(|table| {
                    table
                        .get(key)
                        .map(|derived| (derived.is_fully_scanned(), derived.unpacked_size))
                })
                .unwrap_or((false, None)),
            None => (false, None),
        };
        let unpacked_size = derived_unpacked_size.or(tgz.dist.unpacked_size);
        TarSource {
            name: Some(name),
            tgz_integrity: tgz.integrity_key.clone(),
            keeps_content: keeps_content(self.state, source),
            trust,
            tar_integrity_known,
            decode_limit: tar_decode_limit(unpacked_size),
        }
    }

    /// Serve an upstream version from the dist already read, so the packument
    /// is not parsed a second time.
    async fn serve_upstream(
        &self,
        upstream_name: &str,
        name: &CanonicalPackageName,
        tgz: &VersionTgz,
    ) -> (Response, TgzTrust) {
        let upstream = match authorized_upstream(self.state, self.identity, upstream_name) {
            Ok(upstream) => upstream,
            Err(err) => return (err.into_response(), TgzTrust::Verified),
        };
        let filename = match tarball_cache_name(name, &tgz.filename) {
            Ok((filename, _)) => filename,
            Err(err) => return (err.into_response(), TgzTrust::Verified),
        };
        let integrity = match declared_tarball_integrity(&tgz.dist, name, &filename, self.version) {
            Ok(integrity) => integrity,
            Err(err) => return (err.into_response(), TgzTrust::Verified),
        };
        let namespace = upstream_cache_namespace(self.state, upstream_name);
        let tarball = UpstreamTarball {
            namespace: &namespace,
            name,
            filename: &filename,
            version_route: tgz.version_route.as_deref(),
            integrity: &integrity,
            unpacked_size: tgz.dist.unpacked_size,
        };
        let (response, origin) =
            serve_bound_upstream_tarball(self.state, upstream, tarball, self.version).await;
        let trust = match origin {
            TarballOrigin::Cache => TgzTrust::Cached(CachedTgz { namespace, filename }),
            TarballOrigin::Upstream => TgzTrust::Verified,
        };
        (response, trust)
    }
}

/// Whether the version's source may persist package content: every hosted
/// registry, and an upstream unless it is `cache: false`.
fn keeps_content(state: &AppState, source: &RegistrySource) -> bool {
    match source {
        RegistrySource::Upstream(upstream) => {
            state.inner.proxy.upstreams.get(upstream).is_some_and(pnpr_upstream::Upstream::caches)
        }
        RegistrySource::Hosted(_) => true,
        RegistrySource::Unclaimed | RegistrySource::NotFound => false,
    }
}

/// What an upstream's version manifest says about the requested version.
enum ManifestRead {
    Dist(VersionTgz),
    /// No connection to the upstream, or its circuit is open.
    Unreachable(RegistryError),
    /// No manifest, or none the route can serve, so the packument decides.
    Unusable,
}

/// Where an upstream's cached packument lives, and how long it stays fresh.
struct CachedPackument {
    namespace: String,
    ttl: Duration,
}
