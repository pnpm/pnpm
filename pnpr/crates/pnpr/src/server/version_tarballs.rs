//! The `<name>@<version>` tarball route every registry serves: the published
//! `.tgz`, and on a tar registry the tar it decodes to.
//!
//! A `.tgz` registry serves the version's canonical `.tgz` the way the
//! filename route does, so a warm hit loads no packument. The packument is
//! read only for a version whose `dist.tarball` names another file, and on a
//! tar registry, whose response needs the version's `.tgz` integrity. There
//! the version's dist is kept in a [`VersionTgzCache`].

pub(super) use cache::VersionTgzCache;

use super::{
    Action, AppState, CachedTgz, CanonicalPackageName, DistBlock, Ecosystem, HeaderMap, Identity,
    PackumentDists, RegistryError, RegistrySource, Response, StatusCode, TarSource, TarballOrigin,
    TgzTrust, UpstreamTarball, Value, addressed_registry, authorize, authorized_upstream,
    caller_scoped, declared_tarball_integrity, ensure_osv_allowed, hosted_read_namespace,
    is_tar_registry, not_found, read_source_packument, resolve_registry_source,
    serve_bound_upstream_tarball, serve_registry_tarball, tar_decode_limit, tar_response,
    tarball_basename, tarball_cache_name, tgz_integrity_key, upstream_cache_namespace,
};
use axum::response::IntoResponse;
use std::{sync::Arc, time::Duration};

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
        if !tar_registry && let Some(response) = self.serve_canonical_tgz(&name).await {
            return response;
        }
        let source = resolve_registry_source(self.state, self.target, name.as_str());
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

    /// Serve the version's `.tgz` under its canonical filename, as the filename
    /// route does. `None` when there is none, which leaves a `dist.tarball`
    /// that names another file to the packument.
    async fn serve_canonical_tgz(&self, name: &CanonicalPackageName) -> Option<Response> {
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

    /// The requested version's dist in `source`, from the [`VersionTgzCache`]
    /// or the packument.
    async fn version_tgz(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
    ) -> Result<Option<Arc<VersionTgz>>, RegistryError> {
        let Some((scope, ttl)) = self.packument_scope(source, name)? else { return Ok(None) };
        let cache = &self.state.inner.proxy.version_tgzs;
        if let Some(tgz) = cache.get(&scope, name, self.version) {
            return Ok(Some(tgz));
        }
        let Some(packument) = read_source_packument(self.state, self.identity, source, name).await?
        else {
            return Ok(None);
        };
        let Some(tgz) = version_tgz(&packument, self.version) else { return Ok(None) };
        let tgz = Arc::new(tgz);
        cache.put(scope, name, self.version, Arc::clone(&tgz), ttl);
        Ok(Some(tgz))
    }

    /// Where `source` reads `name`'s packument from, once the caller may read
    /// it there, and how long a version dist read from it stays current: the
    /// packument TTL of an upstream, none for a `cache: false` one, and
    /// [`HOSTED_DIST_TTL`] for a hosted registry.
    fn packument_scope(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
    ) -> Result<Option<(String, Duration)>, RegistryError> {
        match source {
            RegistrySource::Upstream(id) => {
                authorize(self.state, self.identity, source, name.as_str(), Action::Access)?;
                let upstream = authorized_upstream(self.state, self.identity, id)?;
                let ttl = if upstream.caches() {
                    upstream.maxage().unwrap_or(self.state.inner.config.http.packument_ttl)
                } else {
                    Duration::ZERO
                };
                Ok(Some((format!("upstream:{}", upstream_cache_namespace(self.state, id)), ttl)))
            }
            RegistrySource::Hosted(id) => {
                let org = hosted_read_namespace(self.state, self.identity, id, name.as_str())?;
                Ok(Some((format!("hosted:{org}"), HOSTED_DIST_TTL)))
            }
            RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
        }
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

/// How long a hosted version's dist stays in the [`VersionTgzCache`]. A
/// published version's dist does not change, so this bounds only how long an
/// unpublished version stays servable from it.
const HOSTED_DIST_TTL: Duration = Duration::from_secs(30);

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
