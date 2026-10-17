//! The network fetch of a pick and its fallbacks.

use std::sync::Arc;

use pnpm_network::MetadataCacheScope;
use pnpm_registry::{Package, PackageVersion};

use crate::{
    FetchFullMetadataCachedOptions, FetchMetadataError,
    fetch_full_metadata_cached::{
        fetch_full_metadata_bypassing_cache, fetch_full_metadata_cached_lazily,
    },
    metadata_cache_policy::DocumentKind,
    mirror::load_meta_async,
    pick_package_from_meta::RegistryPackageSpec,
};

use super::{
    PackageMetaCache, PickPackageContext, PickPackageError, PickPackageOptions, PickPackageResult,
    PickState, pick_from_meta, route::fetch_failure_falls_back,
};

impl PickState<'_> {
    /// The network fetch via the cached fetcher (step 5).
    pub(super) async fn fetch_and_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        mut disk_meta: Option<Arc<Package>>,
    ) -> Result<PickPackageResult, PickPackageError> {
        let fetch_opts = self.cached_fetch_options(ctx, opts.registry);
        let mut bypass_cache = false;
        loop {
            let meta = match self.fetch_packument(ctx, &spec.name, &fetch_opts, bypass_cache).await
            {
                Ok(meta) => meta,
                Err(error) => {
                    let disk = self.disk_fallback(&error, disk_meta.take()).await;
                    return self.handle_fetch_fallback(error, disk, spec, opts);
                }
            };

            let (meta, picked_meta, picked) = self.upgrade_and_pick(ctx, spec, opts, meta).await?;
            if !picked_meta.versions.corrupt_mirror_fragment_found() {
                self.cache_picked_meta(ctx, opts, &meta);
                return Ok(PickPackageResult { meta: picked_meta, picked_package: picked });
            }
            if bypass_cache {
                return self.unreadable_mirror_error(spec, picked_meta, picked.is_some());
            }
            bypass_cache = true;
        }
    }

    fn unreadable_mirror_error(
        &self,
        spec: &RegistryPackageSpec,
        picked_meta: Arc<Package>,
        has_picked: bool,
    ) -> Result<PickPackageResult, PickPackageError> {
        if !has_picked && picked_meta.latest_decode_error().is_some() {
            return Ok(PickPackageResult { meta: picked_meta, picked_package: None });
        }
        Err(PickPackageError::CorruptMetadataMirror {
            spec_name: spec.name.clone(),
            spec_fetch_spec: spec.fetch_spec.clone(),
            pkg_mirror: self.pkg_mirror.clone().unwrap_or_default(),
        })
    }

    /// A fetch of the full document an earlier release-age upgrade stored in
    /// the abbreviated mirror already sent the request that upgrade would
    /// send, so the packument it returns is marked as checked.
    async fn fetch_packument<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec_name: &str,
        fetch_opts: &FetchFullMetadataCachedOptions<'_>,
        bypass_cache: bool,
    ) -> Result<Arc<Package>, FetchMetadataError> {
        let document = if bypass_cache {
            fetch_full_metadata_bypassing_cache(spec_name, fetch_opts).await
        } else {
            fetch_full_metadata_cached_lazily(spec_name, fetch_opts).await
        }?;
        let meta = Arc::new(document.meta);
        if document.requested == Some(DocumentKind::FullInAbbreviatedMirror) {
            ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(&self.cache_key, &meta);
        }
        Ok(meta)
    }

    async fn upgrade_and_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        meta: Arc<Package>,
    ) -> Result<(Arc<Package>, Arc<Package>, Option<Arc<PackageVersion>>), PickPackageError> {
        let meta = self.upgrade_for_release_age(ctx, spec, opts, meta).await?.meta;
        let (picked_meta, picked) =
            pick_from_meta(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)?;
        Ok((meta, picked_meta, picked))
    }

    fn cache_picked_meta<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        opts: &PickPackageOptions<'_>,
        meta: &Arc<Package>,
    ) {
        if !opts.request.dry_run {
            ctx.metadata.meta_cache.set(self.cache_key.clone(), Arc::clone(meta));
        }
    }

    fn handle_fetch_fallback(
        &self,
        error: FetchMetadataError,
        disk: Option<Arc<Package>>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> Result<PickPackageResult, PickPackageError> {
        let Some(disk) = disk else {
            return Err(error.into());
        };
        tracing::debug!(
            target: "pnpm_resolving_npm_resolver::pick_package",
            ?error,
            pkg_name = %spec.name,
            "metadata fetch failed; falling back to on-disk mirror",
        );
        let (meta, picked) = pick_from_meta(&self.picker_opts, spec, disk, opts.blocked_versions)?;
        if !meta.versions.corrupt_mirror_fragment_found() {
            return Ok(PickPackageResult { meta, picked_package: picked });
        }
        Err(error.into())
    }

    fn cached_fetch_options<'ctx, Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'ctx, Cache>,
        registry: &'ctx str,
    ) -> FetchFullMetadataCachedOptions<'ctx> {
        FetchFullMetadataCachedOptions {
            registry,
            cache_dir: ctx.metadata.cache_dir,
            full_metadata: self.full_metadata,
            filter_metadata: self.use_filtered_full_metadata,
            offline: ctx.cache_policy.network.forbids_registry(),
            priority: pnpm_network::UNPRIORITIZED,
            http: ctx.metadata.http,
        }
    }

    /// The mirror a failed fetch may fall back to, when
    /// [`fetch_failure_falls_back`] allows it. The fallback reads only the
    /// scoped mirror `pkg_mirror` already points at.
    ///
    /// The fetcher already saved a 200 to disk before it returned (when it
    /// returned Ok). If it returned Err, an existing mirror is good enough
    /// to pick from, even if the latest sync failed.
    async fn disk_fallback(
        &self,
        error: &FetchMetadataError,
        disk_meta: Option<Arc<Package>>,
    ) -> Option<Arc<Package>> {
        let public_scope = matches!(self.scope, MetadataCacheScope::Public);
        if !fetch_failure_falls_back(public_scope, error.is_access_denied()) {
            return None;
        }
        match disk_meta {
            Some(meta) => Some(meta),
            None => load_meta_async(self.pkg_mirror.as_deref()).await.map(Arc::new),
        }
    }
}
