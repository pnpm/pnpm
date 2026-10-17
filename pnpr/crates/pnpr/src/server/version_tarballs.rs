//! The `<name>@<version>` tarball route every registry serves: the published
//! `.tgz`, and on a tar registry the tar it decodes to.

use super::{
    AppState, CachedTgz, CanonicalPackageName, DistBlock, Ecosystem, HeaderMap, Identity,
    PackumentDists, RegistrySource, Response, StatusCode, TarSource, TarballOrigin, TgzTrust,
    UpstreamTarball, Value, addressed_registry, authorized_upstream, caller_scoped,
    declared_tarball_integrity, is_tar_registry, not_found, read_source_packument,
    resolve_registry_source, serve_bound_upstream_tarball, serve_registry_tarball, tar_response,
    tarball_basename, tarball_cache_name, tgz_integrity_key, upstream_cache_namespace,
};
use axum::response::IntoResponse;

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
        let source = resolve_registry_source(self.state, self.target, name.as_str());
        let packument = match read_source_packument(self.state, self.identity, &source, &name).await
        {
            Ok(Some(packument)) => packument,
            Ok(None) => return not_found(),
            Err(err) => return err.into_response(),
        };
        let Some(tgz) = version_tgz(&packument, self.version) else {
            return not_found();
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
        if !is_tar_registry(self.state, self.target) || response.status() != StatusCode::OK {
            return response;
        }
        let keeps_content = keeps_content(self.state, &source);
        let source = TarSource { name, tgz_integrity: tgz.integrity_key, keeps_content, trust };
        tar_response(self.state, headers, response, source).await
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
