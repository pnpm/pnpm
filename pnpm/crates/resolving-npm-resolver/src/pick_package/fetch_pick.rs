use super::{
    Arc, FetchFullMetadataCachedOptions, FetchMetadataError, MetadataCacheScope, Package,
    PackageMetaCache, PackageVersion, PickPackageContext, PickPackageError, PickPackageOptions,
    PickPackageResult, PickState, RegistryPackageSpec, UpgradeOutcome,
    fetch_full_metadata_bypassing_cache, fetch_full_metadata_cached, load_meta_async,
    maybe_upgrade_abbreviated_meta_for_release_age, persist_upgraded_to_mirror, pick_from_meta,
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
            let meta = match self.fetch_packument(&spec.name, &fetch_opts, bypass_cache).await {
                Ok(meta) => meta,
                Err(error) => {
                    let disk = self.disk_fallback(&error, disk_meta.take()).await;
                    return self.handle_fetch_fallback(error, disk, spec, opts);
                }
            };

            let (meta, picked_meta, picked) = self.upgrade_and_pick(ctx, spec, opts, meta).await?;
            if !picked_meta.versions.has_corrupt_mirror_fragment() {
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

    async fn fetch_packument(
        &self,
        spec_name: &str,
        fetch_opts: &FetchFullMetadataCachedOptions<'_>,
        bypass_cache: bool,
    ) -> Result<Arc<Package>, FetchMetadataError> {
        let result = if bypass_cache {
            fetch_full_metadata_bypassing_cache(spec_name, fetch_opts).await
        } else {
            fetch_full_metadata_cached(spec_name, fetch_opts).await
        };
        result.map(Arc::new)
    }

    async fn upgrade_and_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        meta: Arc<Package>,
    ) -> Result<(Arc<Package>, Arc<Package>, Option<Arc<PackageVersion>>), PickPackageError> {
        let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
            ctx,
            spec,
            opts,
            self.full_metadata,
            &self.cache_key,
            meta,
        )
        .await?;
        let meta = self.persist_release_age_upgrade(ctx, opts, upgrade);
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
        if !meta.versions.has_corrupt_mirror_fragment() {
            return Ok(PickPackageResult { meta, picked_package: picked });
        }
        Err(error.into())
    }

    fn persist_release_age_upgrade<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        opts: &PickPackageOptions<'_>,
        upgrade: UpgradeOutcome,
    ) -> Arc<Package> {
        let mut meta = upgrade.meta;
        if upgrade.upgraded {
            if !opts.request.dry_run
                && let Some(reloaded) = self.pkg_mirror
                    .as_deref()
                    .and_then(|path| {
                        persist_upgraded_to_mirror(path, &meta, self.use_filtered_full_metadata)
                    })
            {
                meta = Arc::new(reloaded);
            }
            ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(&self.cache_key, &meta);
        }

        meta
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
            offline: ctx.cache_policy.offline,
            priority: pnpm_network::UNPRIORITIZED,
            http: ctx.metadata.http,
        }
    }

    /// The mirror a failed fetch may fall back to.
    ///
    /// The fetcher already saved a 200 to disk before it returned (when it
    /// returned Ok). If it returned Err, an existing mirror is good enough
    /// to pick from, even if the latest sync failed.
    ///
    /// A private route must fail closed on a `401`/`403`/private-`404`: a
    /// revoked credential or a hidden private package must not keep serving
    /// the last cached packument, even from its own (same-namespace) mirror.
    /// Only a transport failure (`5xx`/timeout/network) falls back, and only
    /// within the scoped mirror `pkg_mirror` already points at. A public
    /// route (the CLI / public registries) keeps the original
    /// fall-back-on-any-error behavior.
    async fn disk_fallback(
        &self,
        error: &FetchMetadataError,
        disk_meta: Option<Arc<Package>>,
    ) -> Option<Arc<Package>> {
        let allow_fallback =
            matches!(self.scope, MetadataCacheScope::Public) || !error.is_access_denied();
        if !allow_fallback {
            return None;
        }
        match disk_meta {
            Some(meta) => Some(meta),
            None => load_meta_async(self.pkg_mirror.as_deref()).await.map(Arc::new),
        }
    }
}
