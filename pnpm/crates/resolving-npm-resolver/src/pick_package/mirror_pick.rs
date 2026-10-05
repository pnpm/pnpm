use super::{
    Arc, MetadataCacheScope, Package, PackageMetaCache, PickPackageContext, PickPackageError,
    PickPackageOptions, PickPackageResult, PickState, RegistryPackageSpec, RegistryPackageSpecType,
    TrustPolicy, Utc, cached_meta_misses_preferred_version, dominant_lockfile_version,
    get_file_mtime, load_meta_async, pick_from_meta, pick_from_meta_fast, pick_from_meta_offline,
    pick_stable_cached_range_version,
};
use crate::{
    errors::legacy_mirror_hint,
    mirror::{MetaHeaders, find_legacy_pkg_mirror, load_meta_headers_async},
};

/// Registries that omit `ETag` cannot answer `If-None-Match` with 304, so a
/// warm revalidation downloads the whole packument. A public mirror younger
/// than this and stored without an `ETag` is reused for a range the cache can
/// already satisfy. The public npm registry sends `ETag`s, so it keeps
/// conditional revalidation. After this age the mirror is fetched again.
pub(crate) const UNVALIDATED_MIRROR_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

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
        // A registry that forbade caching must not be answered from the
        // mirror on an online pick. Offline and prefer-offline still may.
        if !ctx.cache_policy.offline
            && !ctx.cache_policy.prefer_offline
            && headers.as_ref().is_some_and(|headers| headers.uncacheable)
        {
            return None;
        }
        if let Some(result) = self.version_spec_pick(ctx, spec, opts, disk_meta).await {
            return Some(result);
        }
        if let Some(result) = self.dominant_version_pick(ctx, spec, opts, disk_meta).await {
            return Some(result);
        }
        if let Some(result) =
            self.fresh_unvalidated_mirror_pick(ctx, spec, opts, headers.as_ref(), disk_meta).await
        {
            return Some(result);
        }
        self.published_by_pick(ctx, spec, opts, disk_meta).await
    }

    /// Version-spec fast path (step 3): the disk cache already has the
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
        if opts.include_latest_tag
            || opts.request.update_checksums
            || !matches!(spec.spec_type, RegistryPackageSpecType::Version)
        {
            return None;
        }
        let meta = self.mirror_meta(disk_meta).await?;
        if !meta.versions.contains_key(&spec.fetch_spec) {
            return None;
        }
        let Ok((picked_meta, Some(picked))) =
            pick_from_meta_fast(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)
        else {
            return None;
        };
        if picked_meta.versions.has_corrupt_mirror_fragment() {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// `true` when the mirror's header line says the last response forbade caching.
    pub(super) async fn mirror_is_uncacheable(&self) -> bool {
        load_meta_headers_async(self.pkg_mirror.as_deref()).await
            .is_some_and(|headers| headers.uncacheable)
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

    /// A range whose lockfile version dominates every other selector can be
    /// answered from the mirror, as long as the mirror's own pick agrees.
    pub(super) async fn dominant_version_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        if !Self::range_pick_is_stable(ctx, spec, opts) {
            return None;
        }
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
        if picked_meta.versions.has_corrupt_mirror_fragment() {
            return None;
        }
        if picked.version.to_string() != stable_version {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// A public mirror with no `ETag` cannot be revalidated cheaply. While it
    /// is younger than [`UNVALIDATED_MIRROR_MAX_AGE`] and already satisfies
    /// the range, skip the full download. A private route still contacts the
    /// registry so a `401` is not hidden behind the mirror.
    pub(super) async fn fresh_unvalidated_mirror_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        headers: Option<&MetaHeaders>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        if !Self::range_can_reuse_unvalidated_mirror(ctx, spec, opts)
            || !matches!(self.scope, MetadataCacheScope::Public)
        {
            return None;
        }
        let headers = headers?;
        if headers.etag
            .as_deref()
            .is_some_and(|etag| !etag.is_empty())
        {
            return None;
        }
        let mtime = self.pkg_mirror.as_deref().and_then(get_file_mtime)?;
        // The age is compared in both directions. A mirror dated far in the
        // future, for example after the clock was set back, has an unknown
        // age and is not reused. A few milliseconds of skew between the file
        // system and the clock are tolerated.
        if Utc::now().signed_duration_since(mtime).abs() >= UNVALIDATED_MIRROR_MAX_AGE {
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
        if picked_meta.versions.has_corrupt_mirror_fragment() {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    fn range_can_reuse_unvalidated_mirror<Cache: PackageMetaCache>(
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> bool {
        Self::range_pick_is_stable(ctx, spec, opts) && !opts.request.refresh_metadata
    }

    /// Whether a range pick could be settled from the mirror at all: every
    /// option that makes the answer depend on fresh metadata rules it out.
    pub(super) fn range_pick_is_stable<Cache: PackageMetaCache>(
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> bool {
        matches!(spec.spec_type, RegistryPackageSpecType::Range)
            && !ctx.cache_policy.offline
            && !ctx.cache_policy.prefer_offline
            && !opts.pick_lowest_version
            && !opts.include_latest_tag
            && !opts.request.update_checksums
            && !opts.policy.release_age_applies_to(&spec.name)
            && opts.policy.trust_policy != Some(TrustPolicy::NoDowngrade)
            && opts.blocked_versions.is_none()
    }

    /// The publishedBy mtime shortcut (step 4): a mirror written after the
    /// cutoff cannot be missing a mature version.
    ///
    /// Fully excluded packages (`minimumReleaseAgeExclude: ['pkg']`) treat
    /// minimumReleaseAge as disabled, so this shortcut must not bypass
    /// revalidation against potentially stale on-disk metadata.
    pub(super) async fn published_by_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: &mut Option<Arc<Package>>,
    ) -> Option<PickPackageResult> {
        if ctx.cache_policy.offline && matches!(spec.spec_type, RegistryPackageSpecType::Range) {
            return None;
        }
        if !opts.policy.release_age_applies_to(&spec.name) {
            return None;
        }
        let published_by = opts.policy.published_by?;
        let mtime = self.pkg_mirror.as_deref().and_then(get_file_mtime)?;
        if mtime < published_by {
            return None;
        }
        let meta = self.mirror_meta(disk_meta).await?;
        let Ok((picked_meta, Some(picked))) =
            pick_from_meta_fast(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)
        else {
            return None;
        };
        if picked_meta.versions.has_corrupt_mirror_fragment() {
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
        if ctx.cache_policy.offline {
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
        let corrupt_mirror = picked_meta.versions.has_corrupt_mirror_fragment();
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
        if meta.versions.has_corrupt_mirror_fragment() {
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
