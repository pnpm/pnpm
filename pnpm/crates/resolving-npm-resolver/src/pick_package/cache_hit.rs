//! The in-memory-cache-hit path of [`super::handle_cache_hit`]: the
//! release-age upgrade, the plain pick, the offline store adjustment, and
//! the registry-unverified guard.

use super::{
    CachedPackument, Package, PackageMetaCache, PackageVersion, PickPackageContext,
    PickPackageError, PickPackageOptions, PickPackageResult, PickState, PickerOpts,
    RegistryPackageSpec, pick_from_meta, pick_from_meta_offline, unverified_pick_is_safe,
};
use std::sync::Arc;

/// Shared cache-hit path. Invoked once on the optimistic pre-permit
/// check and once after the per-key permit is acquired (the re-check
/// that lets duplicate concurrent callers short-circuit without
/// re-fetching). Extracting it keeps the two call sites identical so
/// the upgrade-and-persist side-effects can't drift.
///
/// Returns `Ok(None)` when the hit must not be terminal, and the caller
/// falls through to the disk + network flow that replaces the entry:
/// either the entry is a registry-unverified disk promotion (see
/// [`PackageMetaCache::set_unverified`]) whose pick failed and the
/// resolver isn't offline, or the pick hydrated a damaged fragment of
/// the mirror the entry came from.
pub(super) async fn handle_cache_hit<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    state: &PickState<'_>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    cached: CachedPackument,
) -> Result<Option<PickPackageResult>, PickPackageError> {
    let upgrade = state.upgrade_for_release_age(ctx, spec, opts, cached.meta).await?;
    let upgraded = upgrade.upgraded();
    // The upgrade fetch (re)validated the packument against the registry.
    let registry_verified = cached.registry_verified || upgraded;
    let meta = upgrade.meta;
    if upgraded && !opts.request.dry_run {
        ctx.metadata.meta_cache.set(state.cache_key.clone(), Arc::clone(&meta));
    }
    let picker_opts = &state.picker_opts;
    let cache_key = state.cache_key.as_str();
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
    let (meta, picked) = if ctx.cache_policy.network.forbids_registry() {
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
    if meta.versions.corrupt_mirror_fragment_found() {
        return Ok(None);
    }
    if !ctx.cache_policy.network.forbids_registry()
        && !registry_verified
        && !unverified_pick_is_safe(ctx, spec, opts, &meta, picked.as_ref())
    {
        return Ok(None);
    }
    Ok(Some(PickPackageResult { meta, picked_package: picked }))
}
