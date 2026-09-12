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
//!    [`fetch_full_metadata_cached()`] which threads `If-None-Match`
//!    and `If-Modified-Since` off the mirror's header line.
//! 4. Handing the resolved packument to [`pick_package_from_meta`]
//!    for the actual version pick.
//!
//! Full vs. abbreviated metadata is selected per call from
//! `opts.optional || ctx.full_metadata`. The orchestrator wires the
//! choice through to the
//! mirror directory ([`ABBREVIATED_META_DIR`] vs. [`FULL_META_DIR`]),
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
//! threaded through [`PickPackageContext::fetch_locker`]: the first
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

pub use mirror_persistence::{MirrorPersistError, persist_meta_to_mirror};

pub(crate) use mirror_persistence::{SkippedTimeCheck, warn_missing_time_once};

pub use metadata_cache::{
    CachedPackument, InMemoryPackageMetaCache, PackageMetaCache, PackumentFetchLocker,
    PickedManifestCache, shared_in_memory_cache, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};

mod mirror_pick;

mod mirror_persistence;
use mirror_persistence::{get_file_mtime, metadata_cache_key, validate_package_name};

mod release_age_upgrade;
use release_age_upgrade::{
    UpgradeOutcome, maybe_upgrade_abbreviated_meta_for_release_age, persist_upgraded_to_mirror,
};

mod version_pick;
use version_pick::{PickerOpts, pick_from_meta, pick_from_meta_fast, unverified_pick_is_safe};

mod metadata_cache;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
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
use pnpm_network::{AuthHeaders, MetadataCacheScope, RetryOpts, ThrottledClient};
use pnpm_registry::{Package, PackageVersion};
use pnpm_resolving_resolver_base::{VersionSelectors, parse_packument_timestamp};
use tokio::sync::Semaphore;

use crate::{
    FetchFullMetadataCachedOptions, FetchFullMetadataOptions, FetchFullMetadataOutcome,
    FetchMetadataError, fetch_full_metadata, fetch_full_metadata_cached,
    mirror::{
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, clear_meta,
        get_pkg_mirror_path, load_meta, load_meta_async, save_meta_indexed, save_meta_ndjson,
        scoped_meta_dir,
    },
    pick_package_from_meta::{
        PickPackageFromMetaError, PickPackageFromMetaOptions, RegistryPackageSpec,
        RegistryPackageSpecType, dominant_lockfile_version, filter_pkg_metadata_versions,
        pick_lowest_version_by_version_range, pick_package_from_meta,
        pick_stable_cached_range_version, pick_version_by_version_range,
    },
    registry_url::to_registry_url,
};

/// Process-shared context every [`pick_package`] call reads from.
/// One per install.
pub struct PickPackageContext<'a, Cache: PackageMetaCache> {
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
    pub meta_cache: &'a Cache,
    /// Per-cache-key fetch serializer. See [`PackumentFetchLocker`]
    /// for the rationale. Construct once per install via
    /// [`shared_packument_fetch_locker`] and thread the same handle
    /// through every [`PickPackageContext`] so the npm and named-
    /// registry resolvers coalesce against the same in-flight set.
    pub fetch_locker: &'a PackumentFetchLocker,
    /// Root of the on-disk metadata mirror. `None` disables every
    /// disk path — the orchestrator goes straight to the network.
    pub cache_dir: Option<&'a Path>,
    /// `offline=true` forbids any network access; the picker
    /// surfaces [`PickPackageError::NoOfflineMeta`] when the disk
    /// mirror is also empty.
    pub offline: bool,
    /// `prefer_offline=true` reads disk before the network *and*
    /// returns immediately if disk has a satisfying pick.
    pub prefer_offline: bool,
    /// When [`true`], a `minimumReleaseAge` check that hits an
    /// abbreviated packument (no per-version `time`) warns once and
    /// falls back to picking without the maturity filter.
    ///
    /// Reachable when the registry-served packument omits `time`
    /// even after a full-metadata fetch (rare; the official npm
    /// registry always populates `time` for full responses) — the
    /// opt-in stays for parity with the resolver option flag.
    pub ignore_missing_time_field: bool,
    /// Install-wide bias toward full metadata.
    /// `true` forces every pick to use the full packument; `false`
    /// defers to the per-call `opts.optional` flag, defaulting to
    /// abbreviated metadata. The resolver typically leaves this
    /// `false`; the verifier-time fetcher sets it `true` because
    /// it needs `time` and trust evidence for every entry.
    pub full_metadata: bool,
    /// Asked instead of [`Self::full_metadata`] when the caller can answer
    /// per registry — a registry that declares `supportsTimeField` needs no
    /// full metadata for a time-based resolution even when the others do. The
    /// mirror path and cache key below are already keyed by registry, so two
    /// registries may disagree within one install.
    pub needs_full_metadata_for: Option<&'a (dyn Fn(&str) -> bool + Send + Sync)>,
    /// When full metadata is forced, use pnpm's filtered full-metadata
    /// mirror and filtered packument shape.
    pub filter_metadata: bool,
    /// Retry budget for the picker's metadata fetches. Sourced from
    /// the same `fetch-retries` config the verifier and tarball paths
    /// use, so a registry flap during a pick retries (and a user who
    /// sets `fetch-retries=0` fails fast) exactly as in pnpm.
    pub retry_opts: RetryOpts,
}

/// Per-call options the orchestrator threads to the picker.
pub struct PickPackageOptions<'a> {
    /// Default registry URL for the package (or the per-scope URL
    /// when the package is scoped). The orchestrator stitches this
    /// into the mirror path and the conditional GET URL.
    pub registry: &'a str,
    /// Per-importer version-selector bias.
    pub preferred_version_selectors: Option<&'a VersionSelectors>,
    /// `minimumReleaseAge` cutoff. `None` disables the maturity
    /// filter for this call.
    pub published_by: Option<DateTime<Utc>>,
    /// `minimumReleaseAgeExclude` policy. `None` skips exclusion.
    pub published_by_exclude: Option<&'a PackageVersionPolicy>,
    /// Pick the lowest satisfying version instead of the highest.
    /// Honoured under `published_by` too: maturity narrows which
    /// versions are on offer, and this decides which end of what is
    /// left to take.
    pub pick_lowest_version: bool,
    /// Compare the spec-pick against a `latest`-tag pick and keep
    /// the higher of the two. Used by `pnpm add` to make sure a
    /// freshly-added range picks the same version as the
    /// implicit `@latest` would.
    pub include_latest_tag: bool,
    /// `true` skips the cache write-back on a 200 response — used when
    /// the install is a pure dry-run (`--lockfile-only`, frozen
    /// lockfile, etc.).
    pub dry_run: bool,
    /// `true` forces this pick to use the full packument because
    /// the dependency carries `optionalDependencies`-specific
    /// fields (`libc`, `cpu`, `os`) the abbreviated form drops
    /// some of — see [pnpm/pnpm#9950](https://github.com/pnpm/pnpm/issues/9950).
    /// Combined with [`PickPackageContext::full_metadata`] via OR:
    /// either knob set to `true` makes the pick request full
    /// metadata.
    pub optional: bool,
    /// `true` forces a conditional registry request so a stale disk
    /// packument can't satisfy the call: the on-disk exact-version
    /// fast path is skipped, and the in-memory cache is bypassed too.
    /// The fast path now promotes disk-loaded packuments into the
    /// in-memory cache, so an entry there can no longer be assumed to
    /// come from this install's own fresh network fetch — on a shared
    /// resolver it might be disk-sourced, which would short-circuit the
    /// revalidation. Backs the `--update-checksums` flag.
    pub update_checksums: bool,
    /// Trust-policy validation requires current registry metadata.
    pub trust_policy: Option<TrustPolicy>,
    /// Concrete versions to ignore while picking. Used by callers that
    /// apply an external resolver-time guard: after the guard rejects a
    /// candidate, the caller asks the normal picker to try again over
    /// the same packument with that version filtered out.
    pub blocked_versions: Option<&'a HashSet<String>>,
}

/// Outcome of a successful [`pick_package`] call. `meta` is shared as
/// [`Arc<Package>`] so a hit on the in-memory cache doesn't
/// deep-clone the packument; the upgrade-on-release-age path
/// rebuilds the `Arc` only when it actually replaces the body.
#[derive(Debug)]
pub struct PickPackageResult {
    pub meta: Arc<Package>,
    pub picked_package: Option<Arc<PackageVersion>>,
}

/// Failure modes for [`pick_package`]. Distinguishes the pure-pick
/// errors ([`PickPackageError::Pick`]) from the fetch / IO errors so
/// the install layer can route them through different reporters
/// (a missing time gets a warning; a network failure gets a retry
/// prompt).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PickPackageError {
    /// `ERR_PNPM_INVALID_PACKAGE_NAME`: a package name contains a `/`
    /// but doesn't begin with a `@scope/` prefix.
    #[display("Package name {pkg_name} is invalid, it should have a @scope")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        pkg_name: String,
    },
    /// `ERR_PNPM_NO_OFFLINE_META`: offline mode is active and the
    /// on-disk mirror doesn't have the package.
    #[display("Failed to resolve {spec_name}@{spec_fetch_spec} in package mirror {pkg_mirror:?}")]
    #[diagnostic(code(ERR_PNPM_NO_OFFLINE_META))]
    NoOfflineMeta {
        #[error(not(source))]
        spec_name: String,
        spec_fetch_spec: String,
        pkg_mirror: PathBuf,
    },
    /// Underlying picker error (no versions, unpublished, missing
    /// time, etc.). The picker errors are described on
    /// [`PickPackageFromMetaError`].
    #[diagnostic(transparent)]
    Pick(PickPackageFromMetaError),
    /// Underlying metadata-fetch error (network, decode, 304 with
    /// no cache, etc.). Bubbles up from
    /// [`fetch_full_metadata_cached()`].
    #[diagnostic(transparent)]
    Fetch(FetchMetadataError),
}

impl From<PickPackageFromMetaError> for PickPackageError {
    fn from(error: PickPackageFromMetaError) -> Self {
        PickPackageError::Pick(error)
    }
}

impl From<FetchMetadataError> for PickPackageError {
    fn from(error: FetchMetadataError) -> Self {
        PickPackageError::Fetch(error)
    }
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
/// [`fetch_full_metadata_cached()`], which sends the conditional
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
        let entry = ctx
            .fetch_locker
            .limits
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
    if (ctx.offline || ctx.prefer_offline || opts.pick_lowest_version)
        && let Some(result) = state.offline_disk_pick(ctx, spec, opts, &mut disk_meta).await?
    {
        return Ok(result);
    }

    state.fetch_and_pick(ctx, spec, opts, disk_meta).await
}

/// The route classification and cache keys every layer of one pick shares.
struct PickState<'a> {
    picker_opts: PickerOpts<'a>,
    scope: MetadataCacheScope,
    full_metadata: bool,
    use_filtered_full_metadata: bool,
    pkg_mirror: Option<PathBuf>,
    cache_key: String,
    /// `updateChecksums` must reach the conditional registry request, so it
    /// can't be served from the in-memory cache — which may hold a
    /// disk-promoted entry rather than a fresh network fetch (see the
    /// `update_checksums` doc).
    use_mem_cache: bool,
}

impl<'a> PickState<'a> {
    fn new<Cache: PackageMetaCache>(
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'a>,
    ) -> Self {
        // Every layer below — the in-memory cache, the offline / version-spec
        // / publishedBy disk fast paths, and the network fetch — answers this
        // pick for the same `(registry, package)` route. The fast paths return
        // straight from cache without ever reaching the auth-selection point,
        // so a server route hook would never see this package and its private
        // footprint would under-report the data the resolve depended on.
        // Record the route up front, classified exactly as the network fetch
        // would classify it, so the footprint is complete regardless of which
        // layer serves the metadata. A no-op for the CLI (no hook installed);
        // idempotent on the hook, so the network path re-recording it is fine.
        let url = to_registry_url(opts.registry, &spec.name);
        ctx.auth_headers.record_route(&url, Some(&spec.name));

        // Classify the metadata cache scope once. Every layer below — the
        // in-memory cache, the disk fast paths, and the network fetch — must
        // agree on the mirror namespace and cache keys this route resolves to,
        // or a private packument could leak into (or read from) the global
        // mirror. `Public` for the CLI, leaving the global mirror unchanged.
        let scope = ctx.auth_headers.metadata_scope(&url, Some(&spec.name));

        // The per-registry answer is authoritative when the caller can give
        // one: it already folds in the reasons that hold for every registry,
        // so a registry that carries `time` is free to stay on abbreviated
        // metadata while the others do not.
        let policy_wants_full_metadata = ctx
            .needs_full_metadata_for
            .map_or(ctx.full_metadata, |needs_full_metadata| needs_full_metadata(opts.registry));
        let full_metadata = opts.optional || policy_wants_full_metadata;
        let use_filtered_full_metadata = full_metadata && ctx.filter_metadata;
        let base_meta_dir = if full_metadata {
            if use_filtered_full_metadata { FULL_FILTERED_META_DIR } else { FULL_META_DIR }
        } else {
            ABBREVIATED_META_DIR
        };

        // A `Private` route relocates the mirror under its descriptor
        // namespace so it can never be read by a caller who doesn't reproduce
        // the same descriptor; a `Public` route keeps the global mirror.
        let pkg_mirror = ctx.cache_dir.and_then(|dir| {
            let meta_dir = scoped_meta_dir(&scope, base_meta_dir);
            get_pkg_mirror_path(dir, &meta_dir, opts.registry, &spec.name).ok()
        });

        PickState {
            picker_opts: PickerOpts {
                preferred_version_selectors: opts.preferred_version_selectors,
                published_by: opts.published_by,
                published_by_exclude: opts.published_by_exclude,
                pick_lowest_version: opts.pick_lowest_version,
                include_latest_tag: opts.include_latest_tag,
                ignore_missing_time_field: ctx.ignore_missing_time_field,
            },
            cache_key: metadata_cache_key(
                &scope,
                opts.registry,
                &spec.name,
                full_metadata,
                use_filtered_full_metadata,
            ),
            scope,
            full_metadata,
            use_filtered_full_metadata,
            pkg_mirror,
            use_mem_cache: !opts.update_checksums,
        }
    }

    async fn cached_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> Result<Option<PickPackageResult>, PickPackageError> {
        if !self.use_mem_cache {
            return Ok(None);
        }
        let Some(cached) = ctx.meta_cache.get(&self.cache_key) else {
            return Ok(None);
        };
        handle_cache_hit(
            ctx,
            spec,
            opts,
            &self.picker_opts,
            self.full_metadata,
            self.use_filtered_full_metadata,
            &self.cache_key,
            self.pkg_mirror.as_deref(),
            cached,
        )
        .await
    }

    /// Run the release-age upgrade check over a packument, persisting and
    /// caching the upgraded document when one was fetched.
    async fn upgraded_meta<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        meta: Arc<Package>,
    ) -> Result<Arc<Package>, PickPackageError> {
        let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
            ctx,
            spec,
            opts,
            self.full_metadata,
            &self.cache_key,
            meta,
        )
        .await?;
        let mut meta = upgrade.meta;
        if !upgrade.upgraded {
            // A cache hit re-runs the release-age upgrade check, so serving
            // this meta from memory can't bypass the upgrade.
            self.promote_unverified(ctx, opts, &meta);
            return Ok(meta);
        }
        if !opts.dry_run {
            if let Some(reloaded) = self.pkg_mirror.as_deref().and_then(|path| {
                persist_upgraded_to_mirror(path, &meta, self.use_filtered_full_metadata)
            }) {
                meta = Arc::new(reloaded);
            }
            // The upgrade fetched a registry-validated document; don't
            // downgrade it to an unverified marking.
            ctx.meta_cache.set(self.cache_key.clone(), Arc::clone(&meta));
        }
        Ok(meta)
    }

    /// The network fetch via the cached fetcher (step 5). The cached
    /// fetcher handles conditional headers + 200 cache write internally; on
    /// a 304 it re-reads the mirror body. On the error path, a fetch failure
    /// with a disk fallback uses it; otherwise the error propagates.
    async fn fetch_and_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: Option<Arc<Package>>,
    ) -> Result<PickPackageResult, PickPackageError> {
        let fetch_opts = self.cached_fetch_options(ctx, opts.registry);

        let meta = match fetch_full_metadata_cached(&spec.name, &fetch_opts).await {
            Ok(meta) => Arc::new(meta),
            Err(error) => {
                let Some(disk) = self.disk_fallback(&error, disk_meta).await else {
                    return Err(error.into());
                };
                tracing::debug!(
                    target: "pnpm_resolving_npm_resolver::pick_package",
                    ?error,
                    pkg_name = %spec.name,
                    "metadata fetch failed; falling back to on-disk mirror",
                );
                let (meta, picked) =
                    pick_from_meta(&self.picker_opts, spec, disk, opts.blocked_versions)?;
                return Ok(PickPackageResult { meta, picked_package: picked });
            }
        };

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

        // Worth flagging: a dry-run is meant to gate the on-disk save, but
        // `fetch_full_metadata_cached` already wrote the response body to
        // the mirror by the time it returned, so `opts.dry_run` only
        // suppresses the in-memory cache write. A future refactor that
        // threads `dry_run` into the fetcher can restore a fully
        // no-disk-side-effect dry-run.
        if !opts.dry_run {
            ctx.meta_cache.set(self.cache_key.clone(), Arc::clone(&meta));
        }
        let (meta, picked) = pick_from_meta(&self.picker_opts, spec, meta, opts.blocked_versions)?;
        Ok(PickPackageResult { meta, picked_package: picked })
    }

    fn persist_release_age_upgrade<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        opts: &PickPackageOptions<'_>,
        upgrade: UpgradeOutcome,
    ) -> Arc<Package> {
        let mut meta = upgrade.meta;
        if upgrade.upgraded {
            if !opts.dry_run
                && let Some(reloaded) = self.pkg_mirror.as_deref().and_then(|path| {
                    persist_upgraded_to_mirror(path, &meta, self.use_filtered_full_metadata)
                })
            {
                meta = Arc::new(reloaded);
            }
            ctx.fetch_locker.mark_release_age_upgrade_checked(&self.cache_key, &meta);
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
            http_client: ctx.http_client,
            auth_headers: ctx.auth_headers,
            cache_dir: ctx.cache_dir,
            full_metadata: self.full_metadata,
            filter_metadata: self.use_filtered_full_metadata,
            offline: ctx.offline,
            priority: pnpm_network::UNPRIORITIZED,
            retry_opts: ctx.retry_opts,
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
async fn handle_cache_hit<Cache: PackageMetaCache>(
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
    let mut meta = upgrade.meta;
    // The upgrade fetch (re)validated the packument against the registry.
    let registry_verified = cached.registry_verified || upgrade.upgraded;
    if upgrade.upgraded && !opts.dry_run {
        if let Some(reloaded) = pkg_mirror
            .and_then(|path| persist_upgraded_to_mirror(path, &meta, use_filtered_full_metadata))
        {
            meta = Arc::new(reloaded);
        }
        ctx.meta_cache.set(cache_key.to_string(), Arc::clone(&meta));
    }
    if upgrade.upgraded {
        ctx.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
    }
    let (meta, picked) = pick_from_meta(picker_opts, spec, meta, opts.blocked_versions)?;
    if !ctx.offline
        && !registry_verified
        && !unverified_pick_is_safe(ctx, spec, opts, &meta, picked.as_ref())
    {
        return Ok(None);
    }
    Ok(Some(PickPackageResult { meta, picked_package: picked }))
}

#[cfg(test)]
mod tests;
