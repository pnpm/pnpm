//! Cache+fetch orchestration around [`pick_package_from_meta`].
//!
//! Resolves a [`RegistryPackageSpec`] to a single
//! [`PackageVersion`] by:
//!
//! 1. Consulting an in-memory [`PackageMetaCache`].
//! 2. Falling back to the on-disk JSONL mirror managed by
//!    [`crate::mirror`].
//! 3. Issuing a conditional GET against the registry when neither
//!    cache satisfies the request, using
//!    [`crate::fetch_full_metadata_cached()`] which threads `If-None-Match`
//!    and `If-Modified-Since` off the mirror's header line.
//! 4. Handing the resolved packument to [`pick_package_from_meta`]
//!    for the actual version pick.
//!
//! Full vs. abbreviated metadata is selected per call from
//! `opts.optional || ctx.full_metadata`. The orchestrator wires the
//! choice through to the
//! mirror directory ([`crate::mirror::ABBREVIATED_META_DIR`] vs. [`crate::mirror::FULL_META_DIR`]),
//! the in-memory cache key (`:full` suffix when full), and the
//! `Accept` header on the registry request. When `published_by` is
//! active and the picker ends up with abbreviated metadata that
//! lacks the per-version `time` map, the orchestrator transparently
//! upgrades to full metadata via a follow-up fetch so the
//! `minimumReleaseAge` check runs against real timestamps instead of
//! silently degrading to its warn-and-skip fallback (see
//! [`maybe_upgrade_abbreviated_meta_for_release_age`]).
//!
//! Concurrency: the post-cache-miss flow is serialized per mirror path
//! so concurrent picks for the same package coalesce
//! into a single network fetch — the rest wait, then re-check the
//! in-memory cache that the winner just populated and short-circuit
//! without hitting the registry. This is done via
//! [`PackumentFetchLocker`], which owns per-key semaphores and is
//! threaded through [`MetadataRequestContext::fetch_locker`]: the first
//! caller for a given cache key acquires the per-key permit and
//! does the disk + network work; subsequent callers wait on the
//! permit and re-check
//! [`PackageMetaCache`] after acquiring so the winner's
//! [`PackageMetaCache::set`] short-circuits the rest. Without this,
//! pacquet was firing N concurrent HTTP GETs for the same packument
//! per cluster of cross-referencing deps, queued behind the
//! `ThrottledClient` semaphore — multiplying packument-fetch
//! wall-clock by the dedup factor and putting the resolve walk
//! 3-5× behind pnpm on the `alotta-files` benchmark.

pub use errors::PickPackageError;
pub use mirror_persistence::{MirrorPersistError, persist_meta_to_mirror};
pub use options::{
    MetadataCachePolicy, MetadataPickRequest, MetadataRequestContext, PackagePickPolicy,
    PickPackageContext, PickPackageOptions,
};

pub(crate) use mirror_persistence::{SkippedTimeCheck, warn_missing_time_once};

pub use metadata_cache::{
    CachedPackument, InMemoryPackageMetaCache, PackageMetaCache, PackumentFetchLocker,
    PickedManifestCache, shared_in_memory_cache, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};

pub use offline_store::OfflineStoreView;

mod cache_hit;

mod errors;

mod options;

mod offline_store;
mod state;

mod mirror_pick;

mod mirror_persistence;
use mirror_persistence::{get_file_mtime, metadata_cache_key, validate_package_name};

mod release_age_upgrade;
use release_age_upgrade::{
    UpgradeOutcome, maybe_upgrade_abbreviated_meta_for_release_age, persist_upgraded_to_mirror,
};

mod version_pick;
use cache_hit::handle_cache_hit;
use state::PickState;
use version_pick::{
    PickerOpts, pick_from_meta, pick_from_meta_fast, pick_from_meta_offline,
    unverified_pick_is_safe,
};

mod metadata_cache;

use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{
    TrustPolicy,
    version_policy::{PackageVersionPolicy, PolicyMatch},
};
use pnpm_network::MetadataCacheScope;
use pnpm_registry::{Package, PackageVersion};
use pnpm_resolving_resolver_base::{VersionSelectors, parse_packument_timestamp};
use tokio::sync::Semaphore;

use crate::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, FetchMetadataError,
    mirror::{
        clear_meta, get_pkg_mirror_path, load_meta, load_meta_async, save_meta_indexed,
        save_meta_ndjson,
    },
    pick_package_from_meta::{
        PickPackageFromMetaOptions, RegistryPackageSpec, RegistryPackageSpecType,
        dominant_lockfile_version, filter_pkg_metadata_versions,
        pick_lowest_version_by_version_range, pick_package_from_meta,
        pick_stable_cached_range_version, pick_version_by_version_range,
    },
};

/// Outcome of a successful [`pick_package`] call. `meta` is shared as
/// [`Arc<Package>`] so a hit on the in-memory cache doesn't
/// deep-clone the packument; the upgrade-on-release-age path
/// rebuilds the `Arc` only when it actually replaces the body.
#[derive(Debug)]
pub struct PickPackageResult {
    pub meta: Arc<Package>,
    pub picked_package: Option<Arc<PackageVersion>>,
}

/// Resolve `spec` to a [`PackageVersion`] backed by the registry
/// metadata at `opts.registry`.
///
/// The orchestrator walks four layers before the network:
///
/// 1. **In-memory cache** ([`PackageMetaCache`]).
/// 2. **Offline / pickLowestVersion / preferOffline disk read**.
/// 3. **Version-spec fast path**: if the spec is a pinned version
///    and `include_latest_tag` is off, an on-disk cache that
///    contains that exact version satisfies the call without
///    refetching.
/// 4. **publishedBy mtime shortcut**: if the mirror file was written
///    after the maturity cutoff, reuse it before attempting another
///    conditional fetch.
///
/// Cache-miss / forced-fetch goes through
/// [`crate::fetch_full_metadata_cached()`], which sends the conditional
/// `If-None-Match` / `If-Modified-Since` headers built from the
/// mirror's first line. A 304 reuses the on-disk body.
pub async fn pick_package<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
) -> Result<PickPackageResult, PickPackageError> {
    validate_package_name(&spec.name)?;
    let state = PickState::new(ctx, spec, opts);

    // 1. In-memory cache.
    if let Some(result) = state.cached_pick(ctx, spec, opts).await? {
        return Ok(result);
    }

    // Read-only mirror fast paths, tried *before* the per-name fetch
    // semaphore: a version-pinned pick (or a publishedBy-fresh mirror)
    // needs no fetch exclusivity, and queueing it behind a concurrent
    // fetch of the same name serialized large workspace resolutions
    // behind a handful of slow revalidations. Concurrent same-name
    // callers may briefly duplicate a disk read; the mem-cache
    // promotion inside each path keeps that a one-wave cost.
    let mut disk_meta: Option<Arc<Package>> = None;
    if let Some(result) = state.mirror_fast_paths(ctx, spec, opts, &mut disk_meta).await {
        return Ok(result);
    }

    let limit = {
        let entry = ctx.metadata.fetch_locker.limits
            .entry(state.cache_key.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(1)));
        Arc::clone(entry.value())
    };
    let _permit = limit.acquire().await.expect("packument fetch semaphore should not be closed");
    // The pre-permit fast paths may have read the mirror before the
    // previous permit holder rewrote it; drop that snapshot so every
    // disk-backed pick below reads the current mirror.
    let mut disk_meta: Option<Arc<Package>> = None;

    // Re-check in-memory cache after acquiring the permit — the
    // previous permit holder may have just populated it. Without
    // this re-check, every duplicate caller would still fall
    // through to the disk + network path even though they were
    // waiting precisely for the winner's fetch to complete.
    if let Some(result) = state.cached_pick(ctx, spec, opts).await? {
        return Ok(result);
    }

    // 2. Offline / pickLowestVersion / preferOffline disk read.
    // An online lowest-version pick must not reuse a mirror the registry
    // marked uncacheable. Offline and prefer-offline still may.
    let online_lowest_must_refetch = opts.pick_lowest_version
        && !ctx.cache_policy.offline
        && !ctx.cache_policy.prefer_offline
        && state.mirror_is_uncacheable().await;
    if !online_lowest_must_refetch
        && (ctx.cache_policy.offline || ctx.cache_policy.prefer_offline || opts.pick_lowest_version)
        && let Some(result) = state.offline_disk_pick(ctx, spec, opts, &mut disk_meta).await?
    {
        return Ok(result);
    }

    state.fetch_and_pick(ctx, spec, opts, disk_meta).await
}

#[cfg(test)]
mod tests;
