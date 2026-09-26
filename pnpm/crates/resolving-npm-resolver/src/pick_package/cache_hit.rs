//! The in-memory-cache-hit path of [`super::handle_cache_hit`]: the
//! release-age upgrade, the plain pick, the offline store adjustment, and
//! the registry-unverified guard.

use super::{
    CachedPackument, Package, PackageMetaCache, PackageVersion, PickPackageContext,
    PickPackageError, PickPackageOptions, PickPackageResult, PickerOpts, RegistryPackageSpec,
    UpgradeOutcome, maybe_upgrade_abbreviated_meta_for_release_age, persist_upgraded_to_mirror,
    pick_from_meta, pick_from_meta_offline, unverified_pick_is_safe,
};
use std::{path::Path, sync::Arc};

/// Shared cache-hit path. Invoked once on the optimistic pre-permit
/// check and once after the per-key permit is acquired (the re-check
/// that lets duplicate concurrent callers short-circuit without
/// re-fetching). Extracting it keeps the two call sites identical so
/// the upgrade-and-persist side-effects can't drift.
///
/// Returns `Ok(None)` when the hit must not be terminal: the entry is
/// a registry-unverified disk promotion (see
/// [`PackageMetaCache::set_unverified`]) whose pick failed, and the
/// resolver isn't offline. The caller then falls through to the disk +
/// network flow, whose fetch replaces the entry with a verified one.
///
/// The argument list is wide because the helper consumes everything
/// the per-call frame already computed (cache key, derived
/// `full_metadata`, pre-resolved mirror path, picker options).
/// Bundling these into a struct would just shuffle the same fields
/// into a wrapper without removing any work; allowing the lint is
/// the lower-noise option.
#[expect(
    clippy::too_many_arguments,
    reason = "bundling these independent inputs into a struct moves the fields into a wrapper without removing work"
)]
pub(super) async fn handle_cache_hit<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
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
    let mut meta = Arc::clone(&upgrade.meta);
    // The upgrade fetch (re)validated the packument against the registry.
    let registry_verified = cached.registry_verified || upgrade.upgraded;
    if upgrade.upgraded && !opts.request.dry_run {
        persist_upgrade_to_mirror(
            ctx,
            pkg_mirror,
            use_filtered_full_metadata,
            &upgrade,
            cache_key,
            &mut meta,
        );
    }
    if upgrade.upgraded {
        ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
    }
    let unfiltered_meta = Arc::clone(&meta);
    let (meta, picked) = pick_from_meta(picker_opts, spec, meta, opts.blocked_versions)?;
    finish_cache_hit_pick(
        ctx,
        cache_key,
        picker_opts,
        spec,
        &unfiltered_meta,
        meta,
        picked,
        registry_verified,
        opts,
    )
    .await
}

/// The picks after the release-age upgrade: the plain preference pick, the
/// offline store adjustment over the unfiltered packument, and the
/// registry-unverified guard for non-offline picks.
#[expect(
    clippy::too_many_arguments,
    reason = "bundling these independent inputs into a struct moves the fields into a wrapper without removing work"
)]
async fn finish_cache_hit_pick<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    cache_key: &str,
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    unfiltered_meta: &Arc<Package>,
    meta: Arc<Package>,
    picked: Option<Arc<PackageVersion>>,
    registry_verified: bool,
    opts: &PickPackageOptions<'_>,
) -> Result<Option<PickPackageResult>, PickPackageError> {
    let (meta, picked) = if ctx.cache_policy.offline {
        pick_from_meta_offline(
            ctx.store_view,
            cache_key,
            picker_opts,
            spec,
            unfiltered_meta,
            meta,
            picked,
            opts.blocked_versions,
        )
        .await?
    } else {
        (meta, picked)
    };
    if !ctx.cache_policy.offline
        && !registry_verified
        && !unverified_pick_is_safe(ctx, spec, opts, &meta, picked.as_ref())
    {
        return Ok(None);
    }
    Ok(Some(PickPackageResult { meta, picked_package: picked }))
}

/// Persists a release-age-upgraded packument back to its mirror and the
/// in-memory cache. `meta` is replaced by the reloaded mirror document when
/// the persist produced one.
fn persist_upgrade_to_mirror<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    pkg_mirror: Option<&Path>,
    use_filtered_full_metadata: bool,
    upgrade: &UpgradeOutcome,
    cache_key: &str,
    meta: &mut Arc<Package>,
) {
    if let Some(reloaded) = pkg_mirror.and_then(|path| {
        persist_upgraded_to_mirror(path, meta, use_filtered_full_metadata, upgrade.uncacheable)
    }) {
        *meta = Arc::new(reloaded);
    }
    ctx.metadata.meta_cache.set(cache_key.to_string(), Arc::clone(meta));
}
