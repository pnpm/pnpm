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

/// A cached packument together with its registry-verification state.
/// The two travel as one value so a reader can never pair a packument
/// with the verification state of a concurrent overwrite.
#[derive(Debug, Clone)]
pub struct CachedPackument {
    pub meta: Arc<Package>,
    /// `false` when the packument was parsed straight from the
    /// on-disk mirror without a validating registry round-trip (see
    /// [`PackageMetaCache::set_unverified`]).
    pub registry_verified: bool,
}

/// In-memory packument cache the orchestrator consults before any
/// disk read. A thin map abstraction so a long-lived install can
/// share one cache across many [`pick_package`] calls.
///
/// Implementations must be safe to call concurrently from multiple
/// resolve tasks. The default [`InMemoryPackageMetaCache`] uses a
/// std `Mutex`; a tokio-aware variant can land later if the
/// contention shows up in benchmarks.
pub trait PackageMetaCache: Send + Sync {
    /// Shared handle to the cached packument for `key`, or `None`
    /// when the cache hasn't seen it. The packument is carried as
    /// [`Arc<Package>`] so cross-resolve sharing of a popular
    /// packument (`react`, `lodash`, ...) doesn't deep-clone the
    /// full versions map on every consumer's hit. Returns shared
    /// references, not copies.
    fn get(&self, key: &str) -> Option<CachedPackument>;
    /// Insert/overwrite `meta` under `key`. The orchestrator inserts
    /// after a fresh fetch and after any disk-fast-path that returns
    /// successfully — populating the cache from the disk read avoids
    /// re-paying the `spawn_blocking` + `serde_json::from_str` for
    /// every later resolve of the same `(registry, name)` within the
    /// install. The cache is install-scoped, so a disk-loaded entry
    /// can't outlive the freshness window the disk read already
    /// accepted; the next install starts a fresh cache. Takes
    /// [`Arc<Package>`] so callers can share the same handle they
    /// hand back to [`PickPackageResult`] without an extra clone.
    ///
    /// The caller vouches that `meta` came from (or was revalidated
    /// by) the registry: the entry replaces any registry-unverified
    /// one a previous [`PackageMetaCache::set_unverified`] stored
    /// under `key`.
    fn set(&self, key: String, meta: Arc<Package>);

    /// Like [`PackageMetaCache::set`], but stores the entry as
    /// registry-unverified: it was parsed straight from the on-disk
    /// mirror without a validating registry round-trip, so it may
    /// predate versions the registry has. [`pick_package`] uses the
    /// state to fall through to a conditional registry request —
    /// instead of failing the pick — when a cache hit on such an entry
    /// can't satisfy the requested spec and the resolver isn't offline.
    /// The verified [`PackageMetaCache::set`] the fetch then performs
    /// replaces the entry, so each package revalidates at most once.
    ///
    /// Required (no default) on purpose: an implementation that stored
    /// these entries as verified would silently turn a recoverable
    /// stale-mirror miss into a terminal "no matching version".
    fn set_unverified(&self, key: String, meta: Arc<Package>);
}

/// The packument-fetching state one install owns, shared across every
/// [`PickPackageContext`] in it so the npm and named-registry resolvers
/// coalesce against the same in-flight set.
///
/// Both maps key on the string [`PackageMetaCache`] uses
/// (`{registry}\x00{name}` for abbreviated, `{registry}\x00{name}:full` for
/// full), so two callers asking for different forms of the same packument
/// don't accidentally serialize — the `metaDir` differentiator is embedded
/// in the key.
///
/// The state is install-scoped rather than hung on [`Package`] because a
/// caller may hand the same [`PackageMetaCache`] to install after install,
/// and each of those installs has to make its own decisions.
#[derive(Debug, Default)]
pub struct PackumentFetchState {
    /// One single-permit [`tokio::sync::Semaphore`] per in-memory cache key:
    /// the first caller acquires it and runs the disk-then-network flow,
    /// later ones wait, then re-check [`PackageMetaCache`] and short-circuit
    /// on the hit the winner just wrote, so only the first hits the network.
    limits: DashMap<String, Arc<Semaphore>>,
    /// The packument each cache key's release-age upgrade last got a `304`
    /// for. Holding the document itself (compared by [`Arc::ptr_eq`]) rather
    /// than a bare flag keeps the answer tied to what was revalidated, so a
    /// newer response under the same key is checked on its own.
    release_age_upgrade_checked: DashMap<String, Arc<Package>>,
}

impl PackumentFetchState {
    fn release_age_upgrade_was_checked(&self, cache_key: &str, meta: &Arc<Package>) -> bool {
        self.release_age_upgrade_checked
            .get(cache_key)
            .is_some_and(|checked| Arc::ptr_eq(checked.value(), meta))
    }

    fn mark_release_age_upgrade_checked(&self, cache_key: &str, meta: &Arc<Package>) {
        self.release_age_upgrade_checked.insert(cache_key.to_string(), Arc::clone(meta));
    }
}

pub type PackumentFetchLocker = Arc<PackumentFetchState>;

/// Construct a fresh [`PackumentFetchLocker`] for a new install.
/// Equivalent to `Default::default()`; named for symmetry with
/// [`shared_in_memory_cache`].
#[must_use]
pub fn shared_packument_fetch_locker() -> PackumentFetchLocker {
    Arc::new(PackumentFetchState::default())
}

/// Per-`(registry, pkg_name, version)` cache for the resolver's
/// serialized `manifest` JSON. The npm resolver builds
/// [`pnpm_resolving_resolver_base::ResolveResult`]'s `manifest`
/// field via `serde_json::to_value(picked)`; when many resolves
/// pick the same version of the same package (the common case for
/// shared deps like `react`, `lodash`, ...) every duplicate would
/// otherwise re-walk and re-allocate the same JSON tree. Cache the
/// `Arc<Value>` once per `(registry, pkg_name, version)` triple so
/// the second pick onwards is an `Arc::clone` instead of a full
/// reserialise.
///
/// Shared across [`crate::NpmResolver`] (default + JSR registries)
/// and [`crate::NamedRegistryResolver`] (`<alias>:` specifiers).
/// The key includes `registry` because two registries can serve
/// different artifacts under the same `name@version` — a public
/// `lodash@4.17.21` and a privately-hosted package of the same
/// name-version pair are not interchangeable, and a registry-
/// agnostic key would hand one resolver the other's manifest,
/// breaking the downstream dependency graph / peer extraction /
/// lockfile metadata. Same `{registry}\x00…` scoping shape as
/// [`PackageMetaCache`].
pub type PickedManifestCache = Arc<DashMap<String, Arc<serde_json::Value>>>;

/// Construct a fresh [`PickedManifestCache`] for a new install.
#[must_use]
pub fn shared_picked_manifest_cache() -> PickedManifestCache {
    Arc::new(DashMap::new())
}

/// Default thread-safe [`PackageMetaCache`] backed by a sharded
/// [`DashMap`]. A consumer that already has its own shared map can
/// implement the trait directly instead of using this.
///
/// Every resolve edge consults the cache before anything else, so on
/// a large graph the map takes tens of thousands of lookups from all
/// runtime workers at once — a single `Mutex<HashMap>` here was the
/// top contention point of a warm-resolve time profile.
#[derive(Debug, Default)]
pub struct InMemoryPackageMetaCache {
    inner: DashMap<String, CachedPackument>,
}

impl PackageMetaCache for InMemoryPackageMetaCache {
    fn get(&self, key: &str) -> Option<CachedPackument> {
        self.inner.get(key).map(|entry| entry.value().clone())
    }

    fn set(&self, key: String, meta: Arc<Package>) {
        self.inner.insert(key, CachedPackument { meta, registry_verified: true });
    }

    fn set_unverified(&self, key: String, meta: Arc<Package>) {
        self.inner.insert(key, CachedPackument { meta, registry_verified: false });
    }
}

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

    let picker_opts = PickerOpts {
        preferred_version_selectors: opts.preferred_version_selectors,
        published_by: opts.published_by,
        published_by_exclude: opts.published_by_exclude,
        pick_lowest_version: opts.pick_lowest_version,
        include_latest_tag: opts.include_latest_tag,
        ignore_missing_time_field: ctx.ignore_missing_time_field,
    };

    // The per-registry answer is authoritative when the caller can give one:
    // it already folds in the reasons that hold for every registry, so a
    // registry that carries `time` is free to stay on abbreviated metadata
    // while the others do not.
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

    // A `Private` route relocates the mirror under its descriptor namespace
    // so it can never be read by a caller who doesn't reproduce the same
    // descriptor; a `Public` route keeps the global mirror.
    let pkg_mirror = ctx.cache_dir.and_then(|dir| {
        let meta_dir = scoped_meta_dir(&scope, base_meta_dir);
        get_pkg_mirror_path(dir, &meta_dir, opts.registry, &spec.name).ok()
    });

    let cache_key = metadata_cache_key(
        &scope,
        opts.registry,
        &spec.name,
        full_metadata,
        use_filtered_full_metadata,
    );

    // updateChecksums must reach the conditional registry request below, so it
    // can't be served from the in-memory cache — which may hold a disk-promoted
    // entry rather than a fresh network fetch (see the `update_checksums` doc).
    let use_mem_cache = !opts.update_checksums;

    // 1. In-memory cache.
    if use_mem_cache
        && let Some(cached) = ctx.meta_cache.get(&cache_key)
        && let Some(result) = handle_cache_hit(
            ctx,
            spec,
            opts,
            &picker_opts,
            full_metadata,
            use_filtered_full_metadata,
            &cache_key,
            pkg_mirror.as_deref(),
            cached,
        )
        .await?
    {
        return Ok(result);
    }

    // Read-only mirror fast paths, tried *before* the per-name fetch
    // semaphore: a version-pinned pick (or a publishedBy-fresh mirror)
    // needs no fetch exclusivity, and queueing it behind a concurrent
    // fetch of the same name serialized large workspace resolutions
    // behind a handful of slow revalidations. Concurrent same-name
    // callers may briefly duplicate a disk read; the mem-cache
    // promotion inside each path keeps that a one-wave cost.
    let mut meta_cached_in_store: Option<Arc<Package>> = None;

    // 3. Version-spec fast path.
    if !opts.include_latest_tag
        && !opts.update_checksums
        && matches!(spec.spec_type, RegistryPackageSpecType::Version)
    {
        if meta_cached_in_store.is_none() {
            meta_cached_in_store = load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new);
        }
        if let Some(ref meta) = meta_cached_in_store
            && meta.versions.contains_key(&spec.fetch_spec)
        {
            // The disk cache already has the exact pinned
            // version. The fast picker can throw MissingTime
            // when publishedBy is active and the cache is
            // abbreviated — swallow that and fall through to a
            // network fetch, which would upgrade abbreviated→full.
            // Pacquet's fetcher is always full so this branch
            // shouldn't fire today, but the swallow-and-fall-through
            // keeps the behavior intact.
            if let Ok((picked_meta, Some(picked))) =
                pick_from_meta_fast(&picker_opts, spec, Arc::clone(meta), opts.blocked_versions)
            {
                // Promote the disk-loaded packument into the
                // install-scoped in-memory cache so later resolves
                // for the same `(registry, name)` skip the
                // `spawn_blocking` + multi-MB `serde_json::from_str`
                // this branch just paid. The cache is rebuilt per
                // install, so populating it here can't outlive the
                // freshness window the disk read already accepted —
                // the next install starts a fresh cache and
                // re-evaluates the disk shortcut.
                if !opts.dry_run {
                    ctx.meta_cache.set_unverified(cache_key.clone(), Arc::clone(meta));
                }
                return Ok(PickPackageResult { meta: picked_meta, picked_package: Some(picked) });
            }
        }
    }

    let dominant_lockfile_version = if matches!(spec.spec_type, RegistryPackageSpecType::Range)
        && !ctx.offline
        && !ctx.prefer_offline
        && !opts.pick_lowest_version
        && !opts.include_latest_tag
        && !opts.update_checksums
        && opts.published_by.is_none()
        && opts.trust_policy != Some(TrustPolicy::NoDowngrade)
        && opts.blocked_versions.is_none()
    {
        dominant_lockfile_version(&spec.fetch_spec, opts.preferred_version_selectors)
    } else {
        None
    };
    if dominant_lockfile_version.is_some() {
        if meta_cached_in_store.is_none() {
            meta_cached_in_store = load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new);
        }
        if let Some(ref meta) = meta_cached_in_store
            && let Some(stable_version) = pick_stable_cached_range_version(
                meta,
                &spec.fetch_spec,
                opts.preferred_version_selectors,
            )
            && let Ok((picked_meta, Some(picked))) =
                pick_from_meta_fast(&picker_opts, spec, Arc::clone(meta), None)
            && picked.version.to_string() == stable_version
        {
            if !opts.dry_run {
                ctx.meta_cache.set_unverified(cache_key.clone(), Arc::clone(meta));
            }
            return Ok(PickPackageResult { meta: picked_meta, picked_package: Some(picked) });
        }
    }

    // 4. publishedBy mtime shortcut.
    //
    // Fully excluded packages (`minimumReleaseAgeExclude: ['pkg']`) treat
    // minimumReleaseAge as disabled, so this shortcut must not bypass
    // revalidation against potentially stale on-disk metadata.
    if let Some(published_by) = opts.published_by
        && !matches!(
            opts.published_by_exclude.map(|policy| policy.matches(&spec.name)),
            Some(PolicyMatch::AnyVersion),
        )
        && let Some(mtime) = pkg_mirror.as_deref().and_then(get_file_mtime)
        && mtime >= published_by
    {
        if meta_cached_in_store.is_none() {
            meta_cached_in_store = load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new);
        }
        if let Some(ref meta) = meta_cached_in_store
            && let Ok((picked_meta, Some(picked))) =
                pick_from_meta_fast(&picker_opts, spec, Arc::clone(meta), opts.blocked_versions)
        {
            // Same rationale as the version-spec fast path above —
            // promote the disk-loaded packument into the
            // install-scoped in-memory cache.
            if !opts.dry_run {
                ctx.meta_cache.set(cache_key.clone(), Arc::clone(meta));
            }
            return Ok(PickPackageResult { meta: picked_meta, picked_package: Some(picked) });
        }
    }

    let limit = {
        let entry = ctx
            .fetch_locker
            .limits
            .entry(cache_key.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(1)));
        Arc::clone(entry.value())
    };
    let _permit = limit.acquire().await.expect("packument fetch semaphore should not be closed");
    // The pre-permit fast paths may have read the mirror before the
    // previous permit holder rewrote it; drop that snapshot so every
    // disk-backed pick below reads the current mirror.
    meta_cached_in_store = None;

    // Re-check in-memory cache after acquiring the permit — the
    // previous permit holder may have just populated it. Without
    // this re-check, every duplicate caller would still fall
    // through to the disk + network path even though they were
    // waiting precisely for the winner's fetch to complete.
    if use_mem_cache
        && let Some(cached) = ctx.meta_cache.get(&cache_key)
        && let Some(result) = handle_cache_hit(
            ctx,
            spec,
            opts,
            &picker_opts,
            full_metadata,
            use_filtered_full_metadata,
            &cache_key,
            pkg_mirror.as_deref(),
            cached,
        )
        .await?
    {
        return Ok(result);
    }

    // 2. Offline / pickLowestVersion / preferOffline disk read.
    if ctx.offline || ctx.prefer_offline || opts.pick_lowest_version {
        if meta_cached_in_store.is_none() {
            meta_cached_in_store = load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new);
        }

        if ctx.offline {
            if let Some(meta) = meta_cached_in_store {
                // maybe_upgrade_abbreviated_meta_for_release_age
                // short-circuits when offline, so a later cache hit
                // returns this same meta without any network access.
                if !opts.dry_run {
                    ctx.meta_cache.set_unverified(cache_key.clone(), Arc::clone(&meta));
                }
                let (meta, picked) =
                    pick_from_meta(&picker_opts, spec, meta, opts.blocked_versions)?;
                return Ok(PickPackageResult { meta, picked_package: picked });
            }
            return Err(PickPackageError::NoOfflineMeta {
                spec_name: spec.name.clone(),
                spec_fetch_spec: spec.fetch_spec.clone(),
                pkg_mirror: pkg_mirror.unwrap_or_default(),
            });
        }

        if let Some(meta) = meta_cached_in_store.take() {
            let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
                ctx,
                spec,
                opts,
                full_metadata,
                &cache_key,
                meta,
            )
            .await?;
            let mut meta = upgrade.meta;
            if upgrade.upgraded && !opts.dry_run {
                if let Some(reloaded) = pkg_mirror.as_deref().and_then(|path| {
                    persist_upgraded_to_mirror(path, &meta, use_filtered_full_metadata)
                }) {
                    meta = Arc::new(reloaded);
                }
                ctx.meta_cache.set(cache_key.clone(), Arc::clone(&meta));
            }
            let (picked_meta, picked) =
                pick_from_meta(&picker_opts, spec, Arc::clone(&meta), opts.blocked_versions)?;
            if picked.is_some() {
                // A cache hit re-runs the release-age upgrade check, so
                // serving this meta from memory can't bypass the upgrade.
                // The upgrade branch above already cached the registry-
                // validated document; don't downgrade it to an unverified
                // marking.
                if !upgrade.upgraded && !opts.dry_run {
                    ctx.meta_cache.set_unverified(cache_key.clone(), Arc::clone(&meta));
                }
                return Ok(PickPackageResult { meta: picked_meta, picked_package: picked });
            }
            // Fall through to fetch when disk had the meta but no
            // version satisfied the spec — the disk copy may be
            // stale. Restore the (possibly upgraded) meta for later
            // paths that reuse the in-store load.
            meta_cached_in_store = Some(meta);
        }
    }

    // 5. Network fetch via the cached fetcher. The cached fetcher
    //    handles conditional headers + 200 cache write internally;
    //    on a 304 it re-reads the mirror body. On the error path, if a
    //    fetch failure has a disk fallback we use it; otherwise the
    //    error propagates.
    let fetch_opts = FetchFullMetadataCachedOptions {
        registry: opts.registry,
        http_client: ctx.http_client,
        auth_headers: ctx.auth_headers,
        cache_dir: ctx.cache_dir,
        full_metadata,
        filter_metadata: use_filtered_full_metadata,
        offline: ctx.offline,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: ctx.retry_opts,
    };

    let fetch_result = fetch_full_metadata_cached(&spec.name, &fetch_opts).await;
    let meta = match fetch_result {
        Ok(meta) => Arc::new(meta),
        Err(error) => {
            // The fetcher already saved a 200 to disk before it
            // returned (when it returned Ok). If it returned Err,
            // try the disk fallback: an existing mirror is good
            // enough to pick from, even if the latest sync failed.
            //
            // A private route must fail closed on a `401`/`403`/
            // private-`404`: a revoked credential or a hidden private
            // package must not keep serving the last cached packument,
            // even from its own (same-namespace) mirror. Only a transport
            // failure (`5xx`/timeout/network) falls back, and only within
            // the scoped mirror `pkg_mirror` already points at. A public
            // route (the CLI / public registries) keeps the original
            // fall-back-on-any-error behavior.
            let allow_fallback =
                matches!(scope, MetadataCacheScope::Public) || !error.is_access_denied();
            let disk_fallback = if allow_fallback {
                match meta_cached_in_store {
                    Some(meta) => Some(meta),
                    None => load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new),
                }
            } else {
                None
            };
            if let Some(disk) = disk_fallback {
                tracing::debug!(
                    target: "pnpm_resolving_npm_resolver::pick_package",
                    ?error,
                    pkg_name = %spec.name,
                    "metadata fetch failed; falling back to on-disk mirror",
                );
                let (meta, picked) =
                    pick_from_meta(&picker_opts, spec, disk, opts.blocked_versions)?;
                return Ok(PickPackageResult { meta, picked_package: picked });
            }
            return Err(error.into());
        }
    };

    let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
        ctx,
        spec,
        opts,
        full_metadata,
        &cache_key,
        meta,
    )
    .await?;
    let mut meta = upgrade.meta;
    if upgrade.upgraded
        && !opts.dry_run
        && let Some(reloaded) = pkg_mirror
            .as_deref()
            .and_then(|path| persist_upgraded_to_mirror(path, &meta, use_filtered_full_metadata))
    {
        meta = Arc::new(reloaded);
    }
    if upgrade.upgraded {
        ctx.fetch_locker.mark_release_age_upgrade_checked(&cache_key, &meta);
    }

    // Worth flagging: a dry-run is meant to gate the on-disk save, but
    // `fetch_full_metadata_cached` already wrote the response body to
    // the mirror by the time it returned, so `opts.dry_run` only
    // suppresses the in-memory cache write. A future refactor that
    // threads `dry_run` into the fetcher can restore a fully
    // no-disk-side-effect dry-run.
    //
    if !opts.dry_run {
        ctx.meta_cache.set(cache_key, Arc::clone(&meta));
    }
    let (meta, picked) = pick_from_meta(&picker_opts, spec, meta, opts.blocked_versions)?;
    Ok(PickPackageResult { meta, picked_package: picked })
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
    if !ctx.offline && !registry_verified {
        let stable_cached_range_version =
            if matches!(spec.spec_type, RegistryPackageSpecType::Range)
                && !opts.include_latest_tag
                && !opts.update_checksums
                && opts.published_by.is_none()
                && opts.trust_policy != Some(TrustPolicy::NoDowngrade)
                && opts.blocked_versions.is_none()
            {
                pick_stable_cached_range_version(
                    &meta,
                    &spec.fetch_spec,
                    opts.preferred_version_selectors,
                )
            } else {
                None
            };
        let unverified_pick_is_safe = ctx.prefer_offline
            || opts.pick_lowest_version
            || matches!(spec.spec_type, RegistryPackageSpecType::Version)
            || picked.as_ref().is_some_and(|picked| {
                stable_cached_range_version
                    .as_deref()
                    .is_some_and(|stable| picked.version.to_string() == stable)
            });
        if picked.is_none() || !unverified_pick_is_safe {
            return Ok(None);
        }
    }
    Ok(Some(PickPackageResult { meta, picked_package: picked }))
}

/// Same fields as [`PickPackageOptions`] minus the dispatcher-only
/// ones (registry, `dry_run`); plus the `ignore_missing_time_field`
/// pull-up from the context.
struct PickerOpts<'a> {
    preferred_version_selectors: Option<&'a VersionSelectors>,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&'a PackageVersionPolicy>,
    pick_lowest_version: bool,
    include_latest_tag: bool,
    ignore_missing_time_field: bool,
}

/// Picker that may throw a recoverable
/// [`PickPackageFromMetaError::MissingTime`] — orchestrator callers
/// swallow that on the fast paths so the network fetch can replace
/// abbreviated metadata with full.
fn pick_matching_version_fast(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    if picker_opts.published_by.is_some() {
        pick_respecting_min_release_age(picker_opts, spec, meta)
    } else {
        pick_ignoring_release_age(picker_opts, spec, meta)
    }
}

fn pick_from_meta_fast(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: Arc<Package>,
    blocked_versions: Option<&HashSet<String>>,
) -> Result<(Arc<Package>, Option<Arc<PackageVersion>>), PickPackageFromMetaError> {
    let meta = filter_blocked_versions(meta, blocked_versions);
    if meta.versions.is_empty() && blocked_versions.is_some_and(|blocked| !blocked.is_empty()) {
        return Ok((meta, None));
    }
    let picked = pick_matching_version_fast(picker_opts, spec, &meta)?;
    Ok((meta, picked))
}

fn pick_from_meta(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: Arc<Package>,
    blocked_versions: Option<&HashSet<String>>,
) -> Result<(Arc<Package>, Option<Arc<PackageVersion>>), PickPackageFromMetaError> {
    let meta = filter_blocked_versions(meta, blocked_versions);
    if meta.versions.is_empty() && blocked_versions.is_some_and(|blocked| !blocked.is_empty()) {
        return Ok((meta, None));
    }
    let picked = pick_matching_version_final(picker_opts, spec, &meta)?;
    Ok((meta, picked))
}

fn filter_blocked_versions(
    meta: Arc<Package>,
    blocked_versions: Option<&HashSet<String>>,
) -> Arc<Package> {
    let Some(blocked_versions) = blocked_versions else {
        return meta;
    };
    if blocked_versions.is_empty() {
        return meta;
    }
    Arc::new(filter_pkg_metadata_versions(&meta, |version| !blocked_versions.contains(version)))
}

/// Picker used at terminal return sites where there's no further
/// fall-through. When `ignore_missing_time_field` is on, a
/// [`PickPackageFromMetaError::MissingTime`] surfaces as a one-shot
/// warning and the picker retries without `publishedBy`.
fn pick_matching_version_final(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    match pick_matching_version_fast(picker_opts, spec, meta) {
        Ok(picked) => Ok(picked),
        Err(PickPackageFromMetaError::MissingTime { pkg_name })
            if picker_opts.ignore_missing_time_field =>
        {
            warn_missing_time_once(&pkg_name, SkippedTimeCheck::MinimumReleaseAge);
            let fallback = PickerOpts {
                preferred_version_selectors: picker_opts.preferred_version_selectors,
                published_by: None,
                published_by_exclude: None,
                pick_lowest_version: picker_opts.pick_lowest_version,
                include_latest_tag: picker_opts.include_latest_tag,
                ignore_missing_time_field: picker_opts.ignore_missing_time_field,
            };
            pick_matching_version_fast(&fallback, spec, meta)
        }
        Err(other) => Err(other),
    }
}

/// `publishedBy` is active: it narrows which versions are on offer, and
/// `pick_lowest_version` decides which end of what is left to take. The
/// fallback deliberately drops the maturity filter so a range no mature
/// version satisfies still yields a pick, which the install layer
/// reports as a violation.
fn pick_respecting_min_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    run_picker(picker_opts, spec, |target_spec| {
        let pick_mature = if picker_opts.pick_lowest_version {
            pick_lowest_version_by_version_range
        } else {
            pick_version_by_version_range
        };
        let mature =
            pick_package_from_meta(pick_mature, &meta_opts(picker_opts), meta, target_spec)?;
        if mature.is_some() {
            return Ok(mature);
        }
        let fallback_opts = PickPackageFromMetaOptions {
            preferred_version_selectors: picker_opts.preferred_version_selectors,
            published_by: None,
            published_by_exclude: None,
        };
        pick_package_from_meta(
            pick_lowest_version_by_version_range,
            &fallback_opts,
            meta,
            target_spec,
        )
    })
}

/// `publishedBy` is off: respect `pickLowestVersion`.
fn pick_ignoring_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    run_picker(picker_opts, spec, |target_spec| {
        if picker_opts.pick_lowest_version {
            pick_package_from_meta(
                pick_lowest_version_by_version_range,
                &meta_opts(picker_opts),
                meta,
                target_spec,
            )
        } else {
            pick_package_from_meta(
                pick_version_by_version_range,
                &meta_opts(picker_opts),
                meta,
                target_spec,
            )
        }
    })
}

/// `include_latest_tag` runner. When the flag is off, just delegate
/// to the inner picker. When on, additionally pick against the
/// `latest` tag and return the higher of the two.
fn run_picker<PickOne>(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    pick_one: PickOne,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>
where
    PickOne:
        Fn(&RegistryPackageSpec) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>,
{
    let current = pick_one(spec)?;
    if !picker_opts.include_latest_tag {
        return Ok(current);
    }
    let mut latest_spec = RegistryPackageSpec::latest_tag(spec.name.clone());
    latest_spec.normalized_bare_specifier.clone_from(&spec.normalized_bare_specifier);
    let latest = pick_one(&latest_spec)?;
    Ok(pick_max(current, latest))
}

/// Higher-version-wins between two optional picks. Treats `None`
/// as "no pick" so a single satisfying option wins by default.
fn pick_max(
    lhs: Option<Arc<PackageVersion>>,
    rhs: Option<Arc<PackageVersion>>,
) -> Option<Arc<PackageVersion>> {
    match (lhs, rhs) {
        (None, rhs) => rhs,
        (lhs, None) => lhs,
        (Some(lhs), Some(rhs)) => {
            if lhs.version < rhs.version {
                Some(rhs)
            } else {
                Some(lhs)
            }
        }
    }
}

fn meta_opts<'a>(picker_opts: &'a PickerOpts<'_>) -> PickPackageFromMetaOptions<'a> {
    PickPackageFromMetaOptions {
        preferred_version_selectors: picker_opts.preferred_version_selectors,
        published_by: picker_opts.published_by,
        published_by_exclude: picker_opts.published_by_exclude,
    }
}

/// The in-memory cache + fetch-lock key for a `(registry, package)` pick,
/// namespaced by its [`MetadataCacheScope`].
///
/// A [`MetadataCacheScope::Public`] route keeps the plain
/// `{registry}\x00{name}` key (with the `:full` / `:full:filtered`
/// suffix), so the CLI and public routes are unchanged. A private route
/// prepends its descriptor namespace so one caller's private packument
/// can't satisfy another caller's pick for the same name.
///
/// The registry is part of the key because the same package name can live
/// in two registries (a public `lodash` and a private one); pacquet
/// shares one cache across every pick, so the registry has to be in the
/// key to scope picks per registry. The
/// full-mode suffix keeps a later `optional` pick from reusing an
/// abbreviated entry that dropped `libc`/`cpu`/`os`.
fn metadata_cache_key(
    scope: &MetadataCacheScope,
    registry: &str,
    name: &str,
    full_metadata: bool,
    use_filtered_full_metadata: bool,
) -> String {
    let suffix = if full_metadata {
        if use_filtered_full_metadata { ":full:filtered" } else { ":full" }
    } else {
        ""
    };
    match scope {
        MetadataCacheScope::Public => format!("{registry}\x00{name}{suffix}"),
        MetadataCacheScope::Private { descriptor_id } => {
            format!("private\x00{descriptor_id}\x00{registry}\x00{name}{suffix}")
        }
    }
}

fn validate_package_name(pkg_name: &str) -> Result<(), PickPackageError> {
    // A slash without a `@scope/` prefix is structurally invalid.
    if pkg_name.contains('/') && !pkg_name.starts_with('@') {
        return Err(PickPackageError::InvalidPackageName { pkg_name: pkg_name.to_string() });
    }
    Ok(())
}

fn get_file_mtime(path: &Path) -> Option<DateTime<Utc>> {
    let metadata = std::fs::metadata(path).ok()?;
    let mtime: chrono::DateTime<Utc> = metadata.modified().ok()?.into();
    Some(mtime)
}

/// Time-dependent check a packument with no per-version `time` map
/// takes down. `minimumReleaseAge` and `trustPolicy` both read that one
/// field, so a registry that strips it disables both, and each names
/// itself in the warning it emits.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SkippedTimeCheck {
    #[display("minimumReleaseAge")]
    MinimumReleaseAge,
    #[display("trustPolicy")]
    TrustPolicy,
}

/// Bounded set of `(package name, skipped check)` pairs we've already
/// warned about for the missing-`time` field. Capped at 1024 entries to
/// keep long-lived processes (daemons, store servers) from leaking
/// memory through it.
///
/// Keyed by the pair rather than the package name alone: when both
/// checks are configured they go dark together, and a package-only key
/// would let whichever check ran first silence the other's warning,
/// leaving the user told about only one of the two skips.
///
/// `IndexSet` (not `Vec`) gives O(1) `contains` + cheap insertion-
/// ordered eviction via `shift_remove_index(0)`.
const MAX_WARNED_MISSING_TIME: usize = 1024;
static WARNED_MISSING_TIME: std::sync::LazyLock<
    Mutex<indexmap::IndexSet<(String, SkippedTimeCheck)>>,
> = std::sync::LazyLock::new(|| Mutex::new(indexmap::IndexSet::new()));

pub(crate) fn warn_missing_time_once(pkg_name: &str, skipped_check: SkippedTimeCheck) {
    let mut warned = WARNED_MISSING_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (pkg_name.to_string(), skipped_check);
    if warned.contains(&key) {
        return;
    }
    if warned.len() >= MAX_WARNED_MISSING_TIME {
        // IndexSet preserves insertion order; drop the oldest entry
        // (index 0) so the bound stays at MAX_WARNED_MISSING_TIME.
        warned.shift_remove_index(0);
    }
    warned.insert(key);
    tracing::warn!(
        target: "pnpm_resolving_npm_resolver::pick_package",
        pkg_name,
        r#"The metadata of {pkg_name} is missing the "time" field; skipping the {skipped_check} check for this package."#,
    );
}

/// Convenience writer: persist `meta` to the on-disk mirror under
/// `<cache_dir>/<meta_dir>/<registry>/<encoded-pkg>.jsonl`. Pass
/// [`FULL_META_DIR`] when seeding the full-metadata cache (verifier
/// tests, integrated benchmark) and [`ABBREVIATED_META_DIR`] when
/// seeding the abbreviated cache (resolver tests). Errors are logged
/// at debug by the install path — a cache-write failure should never
/// fail an install. Kept public so the rare caller that
/// constructs a `Package` outside the fetcher (test fixtures, the
/// integrated benchmark's pre-warmer) can seed the mirror without
/// reaching into `crate::mirror`.
pub fn persist_meta_to_mirror(
    cache_dir: &Path,
    meta_dir: &str,
    registry: &str,
    meta: &Package,
) -> Result<(), MirrorPersistError> {
    let path = get_pkg_mirror_path(cache_dir, meta_dir, registry, &meta.name)
        .map_err(|error| MirrorPersistError::EncodePath { error: error.to_string() })?;
    save_meta_indexed(&path, meta, meta.etag.as_deref())
        .map_err(|error| MirrorPersistError::Write { error: error.to_string() })
}

/// Failure modes for [`persist_meta_to_mirror`]. Each variant
/// carries the underlying error as a string because the underlying
/// sources are heterogeneous (`io::Error`, `serde_json::Error`,
/// `EncodeRegistryError`) and the caller only logs.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum MirrorPersistError {
    #[display("Failed to encode mirror path: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_PICK_PACKAGE_ENCODE_PATH))]
    EncodePath {
        #[error(not(source))]
        error: String,
    },
    #[display("Failed to serialize mirror entry: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_PICK_PACKAGE_SERIALIZE))]
    Serialize {
        #[error(not(source))]
        error: String,
    },
    #[display("Failed to write mirror entry: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_PICK_PACKAGE_WRITE))]
    Write {
        #[error(not(source))]
        error: String,
    },
}

/// Shared-state helper that lets a long-running install build one
/// [`PackageMetaCache`] and pass it (by [`Arc`]) to every
/// [`pick_package`] call.
#[must_use]
pub fn shared_in_memory_cache() -> Arc<InMemoryPackageMetaCache> {
    Arc::new(InMemoryPackageMetaCache::default())
}

/// Outcome of [`maybe_upgrade_abbreviated_meta_for_release_age`].
struct UpgradeOutcome {
    /// The packument the orchestrator should pick from. Either the
    /// original meta (no-upgrade arm — same `Arc` as the input) or
    /// a freshly fetched full meta wrapped in a new `Arc`.
    meta: Arc<Package>,
    /// `true` when the orchestrator should persist `meta` to the
    /// abbreviated mirror and write it back to the in-memory cache.
    upgraded: bool,
}

/// Upgrade abbreviated metadata to full when the maturity check needs
/// per-version timestamps.
///
/// When the resolver default-fetched abbreviated metadata but
/// `published_by` is active, the per-version `time` map is missing
/// so the maturity check would silently degrade to the warn-and-skip
/// fallback. This function detects that and re-fetches full metadata
/// when the package's top-level `modified` field shows it was
/// touched after the maturity cutoff. Returns the original meta
/// untouched in every other case.
///
/// The early returns are guard rails:
///
/// - `ctx.offline`: no network allowed. Stick with what we have.
/// - `opts.published_by.is_none()`: maturity check disabled.
/// - `meta.time.is_some()`: the packument carries per-version publish
///   timestamps. Documents whose `time` could not decide maturity are
///   normalized to `None` at the parse boundary (see
///   [`Package::drop_incomplete_publish_times`]), so a map that is here
///   is complete. Nothing to upgrade.
/// - this document already got a `304` from an upgrade fetch earlier in
///   the install (see [`PackumentFetchState`]): the registry has no
///   fuller form of it, so asking again is pure waste.
/// - `opts.published_by_exclude` matches the package: caller has
///   opted this package out of the policy.
/// - `meta.modified.is_some()` and parses as a date `<= cutoff`:
///   every version in the packument was published at or before the
///   cutoff, so the abbreviated form is enough. Inclusive at the
///   boundary on purpose, matching the per-version `<=` filter in
///   [`filter_pkg_metadata_by_publish_date`](crate::filter_pkg_metadata_by_publish_date).
///
/// On upgrade the call uses the network-only [`fetch_full_metadata()`]
/// (not the cached variant) so the response writes back to the
/// abbreviated mirror via [`persist_upgraded_to_mirror`], which
/// intentionally updates the *abbreviated* cache file with full data so
/// the next install sees `time` populated and skips the upgrade fetch.
///
/// The upgrade fetch forwards `meta.etag` and `meta.modified` as
/// conditional headers. When the registry's full-form representation
/// hasn't changed it answers `304 Not Modified` and the abbreviated
/// meta is returned untouched.
async fn maybe_upgrade_abbreviated_meta_for_release_age<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    full_metadata: bool,
    cache_key: &str,
    mut meta: Arc<Package>,
) -> Result<UpgradeOutcome, PickPackageError> {
    if ctx.offline || full_metadata {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    let Some(cutoff) = opts.published_by else {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    };
    if meta.time.is_some() || ctx.fetch_locker.release_age_upgrade_was_checked(cache_key, &meta) {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    if let Some(policy) = opts.published_by_exclude
        && matches!(policy.matches(&spec.name), PolicyMatch::AnyVersion)
    {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    // Inclusive `<=` at the boundary: matches the per-version
    // `<=` filter in `filter_pkg_metadata_by_publish_date`. When
    // `modified` is missing or unparsable we fall through to the
    // upgrade — better to spend one extra fetch than to silently
    // bypass the maturity check.
    if let Some(modified_str) = meta.modified.as_deref()
        && let Some(modified) = parse_packument_timestamp(modified_str)
        && modified <= cutoff
    {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    // One upgrade round trip per document per install: coalesce
    // concurrent callers on a per-key permit (keyed apart from the
    // packument fetch permit, which the network call site holds while
    // calling in here) and let everyone after the first reuse the
    // outcome from the shared cache. Without this, every pick of a
    // popular package repeated the fetch — a large workspace asked the
    // registry for the same packument hundreds of times per install.
    let limit = Arc::clone(
        ctx.fetch_locker
            .limits
            .entry(format!("{cache_key}#release-age-upgrade"))
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .value(),
    );
    let _permit =
        limit.acquire().await.expect("release-age upgrade semaphore should not be closed");
    // Waiting for the permit may have handed the winner's work to us: pick up
    // whatever it left in the cache and re-run the guards before spending a
    // round trip of our own. A checksum refresh skips the cache on purpose —
    // it is the one caller holding a document fresher than the cached one.
    if !opts.update_checksums
        && let Some(cached) = ctx.meta_cache.get(cache_key)
    {
        meta = cached.meta;
    }
    if ctx.fetch_locker.release_age_upgrade_was_checked(cache_key, &meta) || meta.time.is_some() {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    let fetch_opts = FetchFullMetadataOptions {
        registry: opts.registry,
        http_client: ctx.http_client,
        auth_headers: ctx.auth_headers,
        full_metadata: true,
        etag: meta.etag.as_deref(),
        modified: meta.modified.as_deref(),
        retry_opts: ctx.retry_opts,
    };
    match fetch_full_metadata(&spec.name, &fetch_opts).await? {
        FetchFullMetadataOutcome::Modified(upgraded) => {
            Ok(UpgradeOutcome { meta: Arc::new(*upgraded), upgraded: true })
        }
        // 304: the full-form representation matched the conditional
        // headers, so the abbreviated meta is still the freshest
        // signal we have. Keep it (the downstream picker falls through
        // to its warn-and-skip path on the missing `time` map) and
        // mark it so no later pick in this install repeats the round trip.
        // The 304 also registry-validated the document, so it may enter the
        // shared metadata cache as verified.
        FetchFullMetadataOutcome::NotModified => {
            ctx.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
            // A `Modified` outcome is marked by the caller instead: it persists
            // the response to the mirror and may hand back a reloaded document,
            // so only the caller knows the `Arc` that ends up in the cache.
            // Both outcomes must be marked — a registry whose full form is no
            // more complete than its abbreviated one would otherwise be
            // re-asked once per dependency edge.
            if !opts.dry_run {
                ctx.meta_cache.set(cache_key.to_string(), Arc::clone(&meta));
            }
            Ok(UpgradeOutcome { meta, upgraded: false })
        }
    }
}

/// Write the upgraded full metadata back to `pkg_mirror` (which
/// points at the abbreviated cache because the picker is in
/// abbreviated mode). A write failure logs at debug and the install
/// proceeds — the next install simply re-triggers the upgrade fetch.
///
/// On a successful indexed save, returns the just-persisted mirror
/// reloaded in its file-backed form so the caller can cache *it*
/// instead of the response-body-backed document — upgraded packuments
/// are the largest documents an install handles, and caching the
/// in-memory form kept every full body resident for the rest of the
/// resolution.
fn persist_upgraded_to_mirror(
    pkg_mirror: &Path,
    meta: &Package,
    filter_metadata: bool,
) -> Option<Package> {
    let save_result = if filter_metadata {
        let meta_for_cache = match clear_meta(meta) {
            Ok(meta_for_cache) => meta_for_cache,
            Err(error) => {
                tracing::debug!(
                    target: "pnpm_resolving_npm_resolver::pick_package",
                    ?error,
                    path = %pkg_mirror.display(),
                    "could not filter upgraded mirror metadata",
                );
                return None;
            }
        };
        save_meta_ndjson(pkg_mirror, &meta_for_cache, meta.etag.as_deref())
    } else {
        save_meta_indexed(pkg_mirror, meta, meta.etag.as_deref())
    };
    match save_result {
        Ok(()) if !filter_metadata => load_meta(pkg_mirror),
        Ok(()) => None,
        Err(error) => {
            tracing::debug!(
                target: "pnpm_resolving_npm_resolver::pick_package",
                ?error,
                path = %pkg_mirror.display(),
                "could not write upgraded meta to mirror; skipping persist",
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;
