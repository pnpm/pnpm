use super::{
    Arc, DateTime, MetadataCacheScope, Package, PackageMetaCache, PickPackageContext,
    PickPackageError, PickPackageOptions, PickPackageResult, PickState, RegistryPackageSpec, Utc,
    cached_meta_misses_preferred_version, dominant_lockfile_version, get_file_mtime,
    load_meta_async, pick_from_meta, pick_from_meta_fast, pick_from_meta_offline,
    pick_stable_cached_range_version,
    route::{MirrorShortcuts, RouteInputs, RouteRequest, unvalidated_mirror_is_fresh},
};
use crate::{
    errors::legacy_mirror_hint,
    metadata_cache_policy::mirror_may_answer,
    mirror::{MetaHeaders, find_legacy_pkg_mirror, load_meta_headers_async},
};

impl PickState<'_> {
    /// The picks a read-only mirror can answer without taking the fetch
    /// permit.
    pub(super) async fn mirror_fast_paths<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        let headers = load_meta_headers_async(self.pkg_mirror.as_deref()).await;
        let inputs = self.route_inputs(ctx, spec, opts, headers.as_ref());
        let shortcuts = MirrorShortcuts::decide(&inputs)?;
        if shortcuts.exact_version
            && let Some(result) = self.version_spec_pick(ctx, spec, opts, disk_meta).await
        {
            return Some(result);
        }
        if shortcuts.dominant_lockfile_version
            && let Some(result) = self.dominant_version_pick(ctx, spec, opts, disk_meta).await
        {
            return Some(result);
        }
        if shortcuts.unvalidated_mirror
            && let Some(result) =
                self.fresh_unvalidated_mirror_pick(ctx, spec, opts, disk_meta).await
        {
            return Some(result);
        }
        let cutoff = inputs.written_after()?;
        self.published_by_pick(ctx, spec, opts, cutoff, disk_meta).await
    }

    pub(super) fn route_inputs<'inputs, Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &'inputs RegistryPackageSpec,
        opts: &'inputs PickPackageOptions<'inputs>,
        mirror: Option<&'inputs MetaHeaders>,
    ) -> RouteInputs<'inputs> {
        RouteInputs {
            network: ctx.cache_policy.network,
            request: RouteRequest {
                spec_type: spec.spec_type,
                include_latest_tag: opts.include_latest_tag,
                update_checksums: opts.request.update_checksums,
                refresh_metadata: opts.request.refresh_metadata,
                pick_lowest_version: opts.pick_lowest_version,
                has_blocked_versions: opts.blocked_versions.is_some(),
            },
            policy: &opts.policy,
            pkg_name: &spec.name,
            public_scope: matches!(self.scope, MetadataCacheScope::Public),
            mirror,
        }
    }

    /// [`MirrorShortcuts::exact_version`]: the disk cache already has the
    /// exact pinned version.
    ///
    /// The fast picker can throw `MissingTime` when publishedBy is active
    /// and the cache is abbreviated — that is swallowed and falls through to
    /// a network fetch, which would upgrade abbreviated→full. Pacquet's
    /// fetcher is always full so this shouldn't fire today, but the
    /// swallow-and-fall-through keeps the behavior intact.
    pub(super) async fn version_spec_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        let meta = self.mirror_meta(disk_meta).await?;
        if !meta.versions.contains_key(&spec.fetch_spec) {
            return None;
        }
        let Ok((picked_meta, Some(picked))) =
            pick_from_meta_fast(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)
        else {
            return None;
        };
        if picked_meta.versions.corrupt_mirror_fragment_found() {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// [`mirror_may_answer`] over this pick's mirror.
    pub(super) async fn mirror_may_answer<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
    ) -> bool {
        let headers = load_meta_headers_async(self.pkg_mirror.as_deref()).await;
        mirror_may_answer(ctx.cache_policy.network, headers.as_ref())
    }

    /// The mirror, loaded once and reused by every fast path.
    pub(super) async fn mirror_meta(
        &self,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<Arc<Package>> {
        if disk_meta.is_none() {
            *disk_meta = load_meta_async(self.pkg_mirror.as_deref()).await.map(Arc::new);
        }
        disk_meta.clone()
    }

    /// [`MirrorShortcuts::dominant_lockfile_version`], as long as the
    /// mirror's own pick agrees.
    pub(super) async fn dominant_version_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        dominant_lockfile_version(&spec.fetch_spec, opts.preferred_version_selectors)?;
        let meta = self.mirror_meta(disk_meta).await?;
        let stable_version = pick_stable_cached_range_version(
            &meta,
            &spec.fetch_spec,
            opts.preferred_version_selectors,
        )?;
        let Ok((picked_meta, Some(picked))) =
            pick_from_meta_fast(&self.picker_opts, spec, Arc::clone(&meta), None)
        else {
            return None;
        };
        if picked_meta.versions.corrupt_mirror_fragment_found() {
            return None;
        }
        if picked.version.to_string() != stable_version {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// [`MirrorShortcuts::unvalidated_mirror`]: a mirror that already
    /// satisfies the range skips the full download.
    pub(super) async fn fresh_unvalidated_mirror_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        let mtime = self.pkg_mirror.as_deref().and_then(get_file_mtime)?;
        if !unvalidated_mirror_is_fresh(mtime, Utc::now()) {
            return None;
        }
        let meta = self.mirror_meta(disk_meta).await?;
        if cached_meta_misses_preferred_version(
            &meta,
            &spec.fetch_spec,
            opts.preferred_version_selectors,
        ) {
            return None;
        }
        let Ok((picked_meta, Some(picked))) =
            pick_from_meta_fast(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)
        else {
            return None;
        };
        if picked_meta.versions.corrupt_mirror_fragment_found() {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// [`RouteInputs::written_after`]: a mirror written at or after
    /// `cutoff` cannot be missing a mature version.
    ///
    /// Fully excluded packages (`minimumReleaseAgeExclude: ['pkg']`) treat
    /// minimumReleaseAge as disabled, so the route never offers this
    /// shortcut for them: it must not bypass revalidation against
    /// potentially stale on-disk metadata.
    pub(super) async fn published_by_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        cutoff: DateTime<Utc>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        let mtime = self.pkg_mirror.as_deref().and_then(get_file_mtime)?;
        if mtime < cutoff {
            return None;
        }
        let meta = self.mirror_meta(disk_meta).await?;
        let Ok((picked_meta, Some(picked))) =
            pick_from_meta_fast(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)
        else {
            return None;
        };
        if picked_meta.versions.corrupt_mirror_fragment_found() {
            return None;
        }
        // Same rationale as the version-spec fast path — promote the
        // disk-loaded packument into the install-scoped in-memory cache.
        if !opts.request.dry_run {
            ctx.metadata.meta_cache.set(self.cache_key.clone(), meta);
        }
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// Promote a disk-loaded packument into the install-scoped in-memory
    /// cache so later resolves for the same `(registry, name)` skip the
    /// `spawn_blocking` + multi-MB `serde_json::from_str` this pick paid.
    ///
    /// The cache is rebuilt per install, so populating it here can't outlive
    /// the freshness window the disk read already accepted — the next install
    /// starts a fresh cache and re-evaluates the disk shortcut.
    pub(super) fn promote_unverified<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        opts: &PickPackageOptions<'_>,
        meta: &Arc<Package>,
    ) {
        if !opts.request.dry_run {
            ctx.metadata.meta_cache.set_unverified(self.cache_key.clone(), Arc::clone(meta));
        }
    }

    /// The offline / pickLowestVersion / preferOffline disk read (step 2).
    /// `Ok(None)` falls through to the network fetch.
    pub(super) async fn offline_disk_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Result<Option<PickPackageResult>, PickPackageError> {
        let meta = self.mirror_meta(disk_meta).await;
        if ctx.cache_policy.network.forbids_registry() {
            let Some(meta) = meta else {
                return Err(self.no_offline_meta_error(ctx, spec, opts).await);
            };
            return Ok(Some(self.offline_pick(ctx, spec, opts, meta).await?));
        }

        let Some(meta) = meta else { return Ok(None) };
        disk_meta.take();
        let meta = self.upgraded_meta(ctx, spec, opts, meta).await?;
        let (picked_meta, picked) =
            pick_from_meta(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)?;
        let corrupt_mirror = picked_meta.versions.corrupt_mirror_fragment_found();
        if picked.is_some() && !corrupt_mirror {
            return Ok(Some(PickPackageResult { meta: picked_meta, picked_package: picked }));
        }
        if !corrupt_mirror {
            *disk_meta = Some(meta);
        }
        Ok(None)
    }

    async fn no_offline_meta_error(
        &self,
        ctx: &PickPackageContext<'_, impl PackageMetaCache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> PickPackageError {
        let legacy_mirror = match ctx.metadata.cache_dir {
            Some(dir) => {
                find_legacy_pkg_mirror(dir, self.base_meta_dir, opts.registry, &spec.name).await
            }
            None => None,
        };
        let hint = legacy_mirror.map(|path| legacy_mirror_hint(&path));
        PickPackageError::NoOfflineMeta {
            spec_name: spec.name.clone(),
            spec_fetch_spec: spec.fetch_spec.clone(),
            pkg_mirror: self.pkg_mirror.clone().unwrap_or_default(),
            hint,
        }
    }

    /// The offline disk read with its store-aware adjustment.
    async fn offline_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        meta: Arc<Package>,
    ) -> Result<PickPackageResult, PickPackageError> {
        let unfiltered_meta = Arc::clone(&meta);
        let (meta, picked) = pick_from_meta(&self.picker_opts, spec, meta, opts.blocked_versions)?;
        let (meta, picked) = pick_from_meta_offline(
            ctx.store_view,
            &self.cache_key,
            &self.picker_opts,
            spec,
            &unfiltered_meta,
            meta,
            picked,
            opts.blocked_versions,
        )
        .await?;
        if meta.versions.corrupt_mirror_fragment_found() {
            return Err(PickPackageError::NoOfflineMeta {
                spec_name: spec.name.clone(),
                spec_fetch_spec: spec.fetch_spec.clone(),
                pkg_mirror: self.pkg_mirror.clone().unwrap_or_default(),
                hint: None,
            });
        }
        // `maybe_upgrade_abbreviated_meta_for_release_age` short-circuits
        // when offline, so a later cache hit returns this same meta
        // without any network access.
        self.promote_unverified(ctx, opts, &unfiltered_meta);
        Ok(PickPackageResult { meta, picked_package: picked })
    }
}
