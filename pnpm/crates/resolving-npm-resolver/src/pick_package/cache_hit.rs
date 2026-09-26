use super::{
    Arc, CachedPackument, Package, PackageMetaCache, Path, PickPackageContext, PickPackageError,
    PickPackageOptions, PickPackageResult, PickerOpts,
    maybe_upgrade_abbreviated_meta_for_release_age, persist_upgraded_to_mirror, pick_from_meta,
    unverified_pick_is_safe,
};

/// Keep a fetched packument for the next pick in this install, or drop the
/// previous entry when the response forbade caching.
pub(super) fn remember_fetched_meta<Cache: PackageMetaCache>(
    cache: &Cache,
    key: &str,
    meta: &Arc<Package>,
    forbid: bool,
) {
    if forbid {
        cache.remove(key);
        return;
    }
    cache.set(key.to_string(), Arc::clone(meta));
}

/// Shared cache-hit path. Invoked once on the optimistic pre-permit
/// check and once after the per-key permit is acquired.
#[expect(
    clippy::too_many_arguments,
    reason = "bundling these independent inputs into a struct moves the fields into a wrapper without removing work"
)]
pub(super) async fn handle_cache_hit<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &super::RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    picker_opts: &PickerOpts<'_>,
    full_metadata: bool,
    use_filtered_full_metadata: bool,
    cache_key: &str,
    pkg_mirror: Option<&Path>,
    cached: CachedPackument,
) -> Result<Option<PickPackageResult>, PickPackageError> {
    let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
        ctx,
        spec,
        opts,
        full_metadata,
        cache_key,
        cached.meta,
    )
    .await?;
    let mut meta = upgrade.meta;
    let registry_verified = cached.registry_verified || upgrade.upgraded;
    if upgrade.upgraded && !opts.request.dry_run {
        if let Some(reloaded) = pkg_mirror.and_then(|path| {
            persist_upgraded_to_mirror(path, &meta, use_filtered_full_metadata, upgrade.uncacheable)
        }) {
            meta = Arc::new(reloaded);
        }
        remember_fetched_meta(ctx.metadata.meta_cache, cache_key, &meta, upgrade.uncacheable);
    }
    if upgrade.upgraded {
        ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
    }
    let (meta, picked) = pick_from_meta(picker_opts, spec, meta, opts.blocked_versions)?;
    if cache_hit_must_refetch(ctx, spec, opts, &meta, registry_verified, picked.as_ref()) {
        return Ok(None);
    }
    Ok(Some(PickPackageResult { meta, picked_package: picked }))
}

fn cache_hit_must_refetch<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &super::RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    meta: &Arc<Package>,
    registry_verified: bool,
    picked: Option<&Arc<super::PackageVersion>>,
) -> bool {
    !ctx.cache_policy.offline
        && !registry_verified
        && !unverified_pick_is_safe(ctx, spec, opts, meta, picked)
}
