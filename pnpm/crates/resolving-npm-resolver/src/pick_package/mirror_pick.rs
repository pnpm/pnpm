use super::{
    Arc, Package, PackageMetaCache, PickPackageContext, PickPackageError, PickPackageOptions,
    PickPackageResult, PickState, PolicyMatch, RegistryPackageSpec, RegistryPackageSpecType,
    TrustPolicy, dominant_lockfile_version, get_file_mtime, load_meta_async, pick_from_meta,
    pick_from_meta_fast, pick_stable_cached_range_version,
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
        if let Some(result) = self.version_spec_pick(ctx, spec, opts, disk_meta).await {
            return Some(result);
        }
        if let Some(result) = self.dominant_version_pick(ctx, spec, opts, disk_meta).await {
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
            || opts.update_checksums
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
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
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
        if picked.version.to_string() != stable_version {
            return None;
        }
        self.promote_unverified(ctx, opts, &meta);
        Some(PickPackageResult { meta: picked_meta, picked_package: Some(picked) })
    }

    /// Whether a range pick could be settled from the mirror at all: every
    /// option that makes the answer depend on fresh metadata rules it out.
    pub(super) fn range_pick_is_stable<Cache: PackageMetaCache>(
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> bool {
        matches!(spec.spec_type, RegistryPackageSpecType::Range)
            && !ctx.offline
            && !ctx.prefer_offline
            && !opts.pick_lowest_version
            && !opts.include_latest_tag
            && !opts.update_checksums
            && opts.published_by.is_none()
            && opts.trust_policy != Some(TrustPolicy::NoDowngrade)
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
        let published_by = opts.published_by?;
        let fully_excluded = matches!(
            opts.published_by_exclude.map(|policy| policy.matches(&spec.name)),
            Some(PolicyMatch::AnyVersion),
        );
        if fully_excluded {
            return None;
        }
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
        // Same rationale as the version-spec fast path — promote the
        // disk-loaded packument into the install-scoped in-memory cache.
        if !opts.dry_run {
            ctx.meta_cache.set(self.cache_key.clone(), meta);
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
        if !opts.dry_run {
            ctx.meta_cache.set_unverified(self.cache_key.clone(), Arc::clone(meta));
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
        if ctx.offline {
            let Some(meta) = meta else {
                return Err(PickPackageError::NoOfflineMeta {
                    spec_name: spec.name.clone(),
                    spec_fetch_spec: spec.fetch_spec.clone(),
                    pkg_mirror: self.pkg_mirror.clone().unwrap_or_default(),
                });
            };
            // `maybe_upgrade_abbreviated_meta_for_release_age` short-circuits
            // when offline, so a later cache hit returns this same meta
            // without any network access.
            self.promote_unverified(ctx, opts, &meta);
            let (meta, picked) =
                pick_from_meta(&self.picker_opts, spec, meta, opts.blocked_versions)?;
            return Ok(Some(PickPackageResult { meta, picked_package: picked }));
        }

        let Some(meta) = meta else { return Ok(None) };
        disk_meta.take();
        let meta = self.upgraded_meta(ctx, spec, opts, meta).await?;
        let (picked_meta, picked) =
            pick_from_meta(&self.picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)?;
        if picked.is_some() {
            return Ok(Some(PickPackageResult { meta: picked_meta, picked_package: picked }));
        }
        // Fall through to fetch when disk had the meta but no version
        // satisfied the spec — the disk copy may be stale. Restore the
        // (possibly upgraded) meta for later paths that reuse the in-store
        // load.
        *disk_meta = Some(meta);
        Ok(None)
    }
}
