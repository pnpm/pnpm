//! The `<name>@<version>` tarball route every registry serves: the published
//! `.tgz`, and on a tar registry the tar it decodes to.
//!
//! A `.tgz` registry serves a hosted version's blob, or an upstream version's
//! cached `.tgz`, under the canonical filename, so a warm hit loads no
//! packument. Otherwise the route reads the version's dist from the
//! packument, which a tar registry also needs for the `.tgz` integrity. A dist
//! read from a cached upstream packument is kept in a [`VersionTgzCache`].

pub(super) use cache::VersionTgzCache;

use super::{
    Action, AppState, CachedTgz, CanonicalPackageName, DistBlock, Ecosystem, HeaderMap, Identity,
    PackumentDists, RegistryError, RegistrySource, Response, StatusCode, TarSource, TarballOrigin,
    TgzTrust, UpstreamTarball, Value, addressed_registry, authorize, authorized_upstream,
    cached_upstream_tarball, caller_scoped, declared_tarball_integrity, ensure_osv_allowed,
    is_tar_registry, not_found, read_source_packument, resolve_registry_source,
    serve_bound_upstream_tarball, serve_registry_tarball, tar_decode_limit, tar_response,
    tarball_basename, tarball_cache_name, tgz_integrity_key, upstream_cache_namespace,
};
use axum::response::IntoResponse;
use pnpr_storage::DocumentStamp;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

mod cache;

/// `GET {base}/<name>@<version>`: the version's tarball, as the published
/// `.tgz` on a `.tgz` registry and as its tar on a tar registry.
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

    /// Serve the version's `.tgz` under its canonical filename without its
    /// packument: a hosted blob, which publish names canonically, or a cached
    /// upstream `.tgz`, as the filename route serves one. `None` leaves the
    /// version to its packument, including a `dist.tarball` that names
    /// another file.
    async fn serve_canonical_tgz(
        &self,
        name: &CanonicalPackageName,
        source: &RegistrySource,
    ) -> Option<Response> {
        let canonical = name.tarball_name_for_version(self.version);
        match source {
            RegistrySource::Hosted(_) => {
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
                cached_upstream_tarball(self.state, &cached.namespace, name, &canonical).await
            }
            _ => None,
        }
    }

    /// The requested version's dist in `source`, from the [`VersionTgzCache`]
    /// or the packument.
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
        if let (Some(cached), Some(stamp)) = (&cached, stamp)
            && let Some(tgz) = cache.get(&cached.namespace, name, self.version, stamp)
        {
            return Ok(Some(tgz));
        }
        let Some(packument) = read_source_packument(self.state, self.identity, source, name).await?
        else {
            return Ok(None);
        };
        let Some(tgz) = version_tgz(&packument, self.version) else { return Ok(None) };
        let tgz = Arc::new(tgz);
        // The stamp was taken before the read, so a packument rewritten in
        // between only costs the next request a miss.
        if let (Some(cached), Some(stamp)) = (cached, stamp) {
            cache.put(cached.namespace, name, self.version, stamp, Arc::clone(&tgz));
        }
        Ok(Some(tgz))
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
        let tar_integrity_known = match &tgz.integrity_key {
            Some(key) => self.state.inner.storage
                .read_tar_integrities(&name)
                .await
                .is_ok_and(|table| table.contains_key(key)),
            None => false,
        };
        TarSource {
            name,
            tgz_integrity: tgz.integrity_key.clone(),
            keeps_content: keeps_content(self.state, source),
            trust,
            tar_integrity_known,
            decode_limit: tar_decode_limit(tgz.dist.unpacked_size),
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
            integrity: &integrity,
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

/// Where an upstream's cached packument lives, and how long it stays fresh.
struct CachedPackument {
    namespace: String,
    ttl: Duration,
}

struct VersionTgz {
    filename: String,
    dist: DistBlock,
    integrity_key: Option<String>,
}

fn version_tgz(packument: &[u8], version: &str) -> Option<VersionTgz> {
    let mut packument: PackumentDists = serde_json::from_slice(packument).ok()?;
    let dist = packument.versions.shift_remove(version)?.dist?;
    let filename = dist.tarball
        .as_deref()
        .and_then(tarball_basename)?
        .to_string();
    let mut fields = serde_json::Map::new();
    if let Some(integrity) = &dist.integrity {
        fields.insert("integrity".to_string(), Value::String(integrity.clone()));
    }
    if let Some(shasum) = &dist.shasum {
        fields.insert("shasum".to_string(), Value::String(shasum.clone()));
    }
    Some(VersionTgz { filename, integrity_key: tgz_integrity_key(&fields), dist })
}
