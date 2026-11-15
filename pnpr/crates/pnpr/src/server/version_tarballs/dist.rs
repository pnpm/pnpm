use super::{
    Action, CachedPackument, CanonicalPackageName, FetchOutcome, RegistryError, RegistrySource,
    Upstream, VersionRequest, authorized_upstream,
    cache::{DistOrigin, VersionTgz, version_manifest_tgz, version_tgz},
    hosted_read_namespace, timed, upstream_cache_namespace,
};
use crate::server::{authorize, read_source_packument, recover_stale_upstream_packument};
use pnpr_storage::DocumentStamp;
use std::{sync::Arc, time::Instant};

/// The largest upstream version manifest the route reads.
const MAX_VERSION_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

impl VersionRequest<'_> {
    pub(super) fn cached_tgz_is_current(
        &self,
        cached: &CachedPackument,
        name: &CanonicalPackageName,
        tgz: &VersionTgz,
    ) -> bool {
        tgz.is_current(crate::server::upstream_packuments::upstream_package_generation(
            self.state,
            &cached.namespace,
            name,
        ))
    }

    /// The requested version's dist in `source`, from the [`VersionTgzCache`](super::VersionTgzCache)
    /// or the upstream.
    pub(super) async fn version_tgz(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
    ) -> Result<Option<Arc<VersionTgz>>, RegistryError> {
        let cached = self.cached_packument(source, name)?;
        let generation = cached
            .as_ref()
            .map(|cached| {
                crate::server::upstream_packuments::upstream_package_generation(
                    self.state,
                    &cached.namespace,
                    name,
                )
            });
        let stamp = match &cached {
            Some(cached) => self.fresh_packument_stamp(cached, name).await?,
            None => None,
        };
        let cache = &self.state.inner.proxy.version_tgzs;
        if let Some(cached) = &cached
            && let Some(tgz) = cache.get(&cached.namespace, name, self.version, stamp, cached.ttl)
            && generation.is_some_and(|generation| tgz.is_current(generation))
        {
            return Ok(Some(tgz));
        }
        let Some((mut tgz, origin)) =
            self.read_version_tgz(source, name, cached.as_ref(), stamp).await?
        else {
            return Ok(None);
        };
        if matches!(origin, Some(DistOrigin::Packument(_))) {
            tgz.packument_generation = generation;
        }
        let tgz = Arc::new(tgz);
        if let (Some(cached), Some(origin)) = (cached, origin) {
            cache.put(cached.namespace, name, self.version, origin, Arc::clone(&tgz));
        }
        Ok(Some(tgz))
    }

    /// Read the version's dist and say where it came from, when it may be
    /// cached. Without a cached packument, an upstream's version
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
        if let RegistrySource::Hosted(hosted) = source {
            let namespace =
                hosted_read_namespace(self.state, self.identity, hosted, name.as_str())?;
            let package = name.clone();
            let version = self.version.to_string();
            let tgz = self.state.inner.storage
                .for_hosted(&namespace, pnpr_registry::Ecosystem::Npm)
                .lookup_in_hosted_document(name, move |bytes| {
                    Ok(version_tgz(bytes, &package, &version))
                })
                .await?
                .found();
            return Ok(tgz.map(|tgz| (tgz, None)));
        }
        if stamp.is_none()
            && self.has_no_cached_packument(cached, name).await?
            && let RegistrySource::Upstream(id) = source
            && let Some(read) = self.read_version_manifest(id, name, cached).await
        {
            return Ok(Some(read));
        }
        let Some(packument) = read_source_packument(self.state, self.identity, source, name).await?
        else {
            return Ok(None);
        };
        let packument = self.wait_for_packument_pin(source, name, cached, packument).await?;
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

    async fn has_no_cached_packument(
        &self,
        cached: Option<&CachedPackument>,
        name: &CanonicalPackageName,
    ) -> Result<bool, RegistryError> {
        let Some(cached) = cached else { return Ok(true) };
        Ok(self.state.inner.storage
            .upstream_document_stamp(&cached.namespace, name)
            .await?
            .is_none())
    }

    async fn wait_for_packument_pin(
        &self,
        source: &RegistrySource,
        name: &CanonicalPackageName,
        cached: Option<&CachedPackument>,
        mut packument: Vec<u8>,
    ) -> Result<Vec<u8>, RegistryError> {
        if let Some(tgz) = version_tgz(&packument, name, self.version)
            && tgz.integrity_key.is_none()
            && let Some(cached) = cached
        {
            if let RegistrySource::Upstream(id) = source {
                let upstream = authorized_upstream(self.state, self.identity, id)?;
                crate::server::upstream_integrity::wait_for_pin(
                    self.state,
                    &cached.namespace,
                    &upstream,
                    name,
                    &tgz.filename,
                )
                .await?;
            }
            if let Some(pinned) =
                self.state.inner.storage.read_upstream_document_any(&cached.namespace, name).await?
            {
                packument = pinned;
            }
        }
        Ok(packument)
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
        let generation = cached.map(|cached| {
            crate::server::upstream_packuments::upstream_package_generation(
                self.state,
                &cached.namespace,
                name,
            )
        });
        let fetched = Instant::now();
        let err = match self.fetch_version_manifest(&upstream, name).await {
            ManifestRead::Dist(mut tgz) => {
                tgz.manifest_generation = generation;
                return Some((tgz, Some(DistOrigin::VersionManifest(fetched))));
            }
            ManifestRead::Unreachable(err) => err,
            ManifestRead::Unusable => return None,
        };
        // A packument fetch would wait on the same upstream again.
        let stale =
            recover_stale_upstream_packument(self.state, &cached?.namespace, &upstream, name, err)
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
    pub(super) fn cached_packument(
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
            ttl: upstream.packument_ttl().unwrap_or(self.state.inner.config.http.packument_ttl),
        }))
    }

    /// The stamp of the cached packument, while it is fresh enough to serve.
    pub(super) async fn fresh_packument_stamp(
        &self,
        cached: &CachedPackument,
        name: &CanonicalPackageName,
    ) -> Result<Option<DocumentStamp>, RegistryError> {
        self.state.inner.storage.fresh_upstream_document_stamp(&cached.namespace, name, cached.ttl)
            .await
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
