//! Cache+fetch orchestration around [`pick_package_from_meta`].
//!
//! Ports pnpm's
//! [`pickPackage.ts`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts).
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
//! `opts.optional || ctx.full_metadata`, matching upstream's
//! [`fullMetadata`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L201)
//! derivation. The orchestrator wires the choice through to the
//! mirror directory ([`ABBREVIATED_META_DIR`] vs. [`FULL_META_DIR`]),
//! the in-memory cache key (`:full` suffix when full), and the
//! `Accept` header on the registry request. When `published_by` is
//! active and the picker ends up with abbreviated metadata that
//! lacks the per-version `time` map, the orchestrator transparently
//! upgrades to full metadata via a follow-up fetch so the
//! `minimumReleaseAge` check runs against real timestamps instead of
//! silently degrading to its warn-and-skip fallback. Ports upstream's
//! [`maybeUpgradeAbbreviatedMetaForReleaseAge`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L450-L501).
//!
//! Concurrency: upstream's
//! [`runLimited(pkgMirror, …)`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L52)
//! wraps the post-cache-miss flow in a `pLimit(1)` keyed by the
//! mirror path so concurrent picks for the same package coalesce
//! into a single network fetch — the rest wait, then re-check the
//! in-memory cache that the winner just populated and short-circuit
//! without hitting the registry. Pacquet ports this via
//! [`PackumentFetchLocker`], a [`DashMap<String, Arc<Semaphore>>`]
//! threaded through [`PickPackageContext::fetch_locker`]: the first
//! caller for a given cache key acquires the per-key permit and
//! does the disk + network work; subsequent callers wait on the
//! permit and (per pnpm's runLimited semantics) re-check
//! [`PackageMetaCache`] after acquiring so the winner's
//! [`PackageMetaCache::set`] short-circuits the rest. Without this,
//! pacquet was firing N concurrent HTTP GETs for the same packument
//! per cluster of cross-referencing deps, queued behind the
//! `ThrottledClient` semaphore — multiplying packument-fetch
//! wall-clock by the dedup factor and putting the resolve walk
//! 3-5× behind pnpm on the `alotta-files` benchmark.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::version_policy::{PackageVersionPolicy, PolicyMatch};
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_registry::{Package, PackageVersion};
use pacquet_resolving_resolver_base::{VersionSelectors, parse_packument_timestamp};
use tokio::sync::Semaphore;

use crate::{
    FetchFullMetadataCachedOptions, FetchFullMetadataOptions, FetchFullMetadataOutcome,
    FetchMetadataError, fetch_full_metadata, fetch_full_metadata_cached,
    mirror::{
        ABBREVIATED_META_DIR, FULL_META_DIR, get_pkg_mirror_path, load_meta_async,
        save_meta_indexed,
    },
    pick_package_from_meta::{
        PickPackageFromMetaError, PickPackageFromMetaOptions, RegistryPackageSpec,
        RegistryPackageSpecType, pick_lowest_version_by_version_range, pick_package_from_meta,
        pick_version_by_version_range,
    },
};

/// In-memory packument cache the orchestrator consults before any
/// disk read. Mirrors upstream's
/// [`PackageMetaCache`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L27-L31)
/// interface — a thin map abstraction so a long-lived install can
/// share one cache across many [`pick_package`] calls.
///
/// Implementations must be safe to call concurrently from multiple
/// resolve tasks. The default [`InMemoryPackageMetaCache`] uses a
/// std `Mutex`; a tokio-aware variant can land later if the
/// contention shows up in benchmarks.
pub trait PackageMetaCache: Send + Sync {
    /// Shared handle to the cached packument for `key`, or `None`
    /// when the cache hasn't seen it. Returned as
    /// [`Arc<Package>`] so cross-resolve sharing of a popular
    /// packument (`react`, `lodash`, ...) doesn't deep-clone the
    /// full versions map on every consumer's hit. Mirrors JS
    /// `Map.get` semantics — pnpm's metaCache returns object
    /// references, not copies, and pacquet matches that contract.
    fn get(&self, key: &str) -> Option<Arc<Package>>;
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
    fn set(&self, key: String, meta: Arc<Package>);
}

/// Per-`(registry, package_name)` fetch serializer. Mirrors
/// upstream's [`metafileOperationLimits`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L42-L44)
/// — a map of `pLimit(1)` instances keyed on the on-disk mirror
/// path, used by [`runLimited`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L52-L64)
/// to coalesce concurrent picks for the same packument.
///
/// Pacquet stores one [`tokio::sync::Semaphore`] (single-permit) per
/// in-memory cache key; first caller acquires, runs the
/// disk-then-network flow, releases. Subsequent callers wait on the
/// same semaphore; after acquiring, they re-check
/// [`PackageMetaCache`] and short-circuit on a hit so only the first
/// caller hits the network.
///
/// Shared across every [`PickPackageContext`] in a single install so
/// the npm and named-registry resolvers coalesce against the same
/// in-flight set. Keying is on the same string [`PackageMetaCache`]
/// uses (`{registry}\x00{name}` for abbreviated,
/// `{registry}\x00{name}:full` for full), so two callers asking for
/// different forms of the same packument don't accidentally serialize
/// — pnpm scopes its `runLimited` the same way (per `pkgMirror`,
/// which embeds the `metaDir` differentiator).
pub type PackumentFetchLocker = Arc<DashMap<String, Arc<Semaphore>>>;

/// Construct a fresh [`PackumentFetchLocker`] for a new install.
/// Equivalent to `Default::default()`; named for symmetry with
/// [`shared_in_memory_cache`].
#[must_use]
pub fn shared_packument_fetch_locker() -> PackumentFetchLocker {
    Arc::new(DashMap::new())
}

/// Per-`(registry, pkg_name, version)` cache for the resolver's
/// serialized `manifest` JSON. The npm resolver builds
/// [`pacquet_resolving_resolver_base::ResolveResult`]'s `manifest`
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
    inner: DashMap<String, Arc<Package>>,
}

impl PackageMetaCache for InMemoryPackageMetaCache {
    fn get(&self, key: &str) -> Option<Arc<Package>> {
        self.inner.get(key).map(|entry| Arc::clone(entry.value()))
    }

    fn set(&self, key: String, meta: Arc<Package>) {
        self.inner.insert(key, meta);
    }
}

/// Process-shared context every [`pick_package`] call reads from.
/// One per install. Mirrors the upstream
/// [`ctx`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L172-L182)
/// parameter.
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
    /// mirror is also empty. Mirrors upstream's `ctx.offline`.
    pub offline: bool,
    /// `prefer_offline=true` reads disk before the network *and*
    /// returns immediately if disk has a satisfying pick. Mirrors
    /// upstream's `ctx.preferOffline`.
    pub prefer_offline: bool,
    /// When [`true`], a `minimumReleaseAge` check that hits an
    /// abbreviated packument (no per-version `time`) warns once and
    /// falls back to picking without the maturity filter. Mirrors
    /// upstream's `ctx.ignoreMissingTimeField`.
    ///
    /// Reachable when the registry-served packument omits `time`
    /// even after a full-metadata fetch (rare; the official npm
    /// registry always populates `time` for full responses) — the
    /// opt-in stays for parity with the resolver option flag.
    pub ignore_missing_time_field: bool,
    /// Install-wide bias toward full metadata, mirroring upstream's
    /// [`ctx.fullMetadata`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L175).
    /// `true` forces every pick to use the full packument; `false`
    /// defers to the per-call `opts.optional` flag, defaulting to
    /// abbreviated metadata. The resolver typically leaves this
    /// `false`; the verifier-time fetcher sets it `true` because
    /// it needs `time` and trust evidence for every entry.
    pub full_metadata: bool,
    /// Retry budget for the picker's metadata fetches. Sourced from
    /// the same `fetch-retries` config the verifier and tarball paths
    /// use, so a registry flap during a pick retries (and a user who
    /// sets `fetch-retries=0` fails fast) exactly as in pnpm.
    pub retry_opts: RetryOpts,
}

/// Per-call options the orchestrator threads to the picker. Mirrors
/// upstream's
/// [`PickPackageOptions`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L66-L73).
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
    /// Mirrors `pickLowestVersion` on the upstream call site, and
    /// is forced to `false` when `published_by` is active (the
    /// maturity filter always picks highest then falls back to
    /// lowest).
    pub pick_lowest_version: bool,
    /// Compare the spec-pick against a `latest`-tag pick and keep
    /// the higher of the two. Used by `pnpm add` to make sure a
    /// freshly-added range picks the same version as the
    /// implicit `@latest` would.
    pub include_latest_tag: bool,
    /// `true` skips the cache write-back on a 200 response.
    /// Matches the upstream flag — used when the install is a
    /// pure dry-run (`--lockfile-only`, frozen lockfile, etc.).
    pub dry_run: bool,
    /// `true` forces this pick to use the full packument because
    /// the dependency carries `optionalDependencies`-specific
    /// fields (`libc`, `cpu`, `os`) the abbreviated form drops
    /// some of. Mirrors upstream's
    /// [`opts.optional`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L72)
    /// — see [pnpm/pnpm#9950](https://github.com/pnpm/pnpm/issues/9950).
    /// Combined with [`PickPackageContext::full_metadata`] via OR:
    /// either knob set to `true` makes the pick request full
    /// metadata.
    pub optional: bool,
    /// `true` skips the on-disk exact-version fast path so a stale
    /// disk packument can't satisfy the call without a conditional
    /// registry request. Mirrors pnpm's `--update-checksums`.
    pub update_checksums: bool,
}

/// Outcome of a successful [`pick_package`] call. Mirrors
/// upstream's `{ meta, pickedPackage }`. `meta` is shared as
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
    /// Mirrors upstream's `ERR_PNPM_INVALID_PACKAGE_NAME`. Triggers
    /// when a package name contains a `/` but doesn't begin with a
    /// `@scope/` prefix.
    #[display("Package name {pkg_name} is invalid, it should have a @scope")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        pkg_name: String,
    },
    /// Mirrors upstream's
    /// [`ERR_PNPM_NO_OFFLINE_META`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L242).
    /// Offline mode is active and the on-disk mirror doesn't have
    /// the package.
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
/// metadata at `opts.registry`. Mirrors upstream's
/// [`pickPackage`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L172-L432).
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
///    conditional fetch. This mirrors pnpm's cache freshness shortcut.
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

    let picker_opts = PickerOpts {
        preferred_version_selectors: opts.preferred_version_selectors,
        published_by: opts.published_by,
        published_by_exclude: opts.published_by_exclude,
        pick_lowest_version: opts.pick_lowest_version,
        include_latest_tag: opts.include_latest_tag,
        ignore_missing_time_field: ctx.ignore_missing_time_field,
    };

    // Per upstream's
    // [`pickPackage`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L201):
    // `opts.optional` is a per-call escape hatch (needed for
    // `libc`/`cpu`/`os` filtering on optional deps —
    // <https://github.com/pnpm/pnpm/issues/9950>); `ctx.full_metadata`
    // is the install-wide bias. Either being `true` forces the full
    // packument.
    let full_metadata = opts.optional || ctx.full_metadata;
    let meta_dir = if full_metadata { FULL_META_DIR } else { ABBREVIATED_META_DIR };

    let pkg_mirror = ctx
        .cache_dir
        .and_then(|dir| get_pkg_mirror_path(dir, meta_dir, opts.registry, &spec.name).ok());

    // Scope the in-memory cache key by registry so the same package
    // name in two different registries (private + public, scoped
    // override, etc.) never short-circuits to the wrong packument.
    // Upstream pnpm gets the same scoping by holding one
    // `PackageMetaCache` per resolver instance per registry; pacquet
    // shares one cache across all `pick_package` calls, so the key
    // has to do the scoping itself.
    //
    // The `:full` suffix mirrors upstream's
    // [cache-key shape](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L206):
    // a later call with `opts.optional = true` must not satisfy
    // itself with the abbreviated cache entry an earlier call
    // populated (the abbreviated form drops `libc`/`cpu`/`os` from
    // some shapes).
    let cache_key = if full_metadata {
        format!("{}\x00{}:full", opts.registry, spec.name)
    } else {
        format!("{}\x00{}", opts.registry, spec.name)
    };

    // 1. In-memory cache.
    if let Some(cached) = ctx.meta_cache.get(&cache_key) {
        return handle_cache_hit(
            ctx,
            spec,
            opts,
            &picker_opts,
            full_metadata,
            &cache_key,
            pkg_mirror.as_deref(),
            cached,
        )
        .await;
    }

    // Per-cache-key fetch serializer. Mirrors upstream's
    // [`runLimited(pkgMirror, ...)`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L52-L64)
    // pLimit(1): concurrent picks for the same packument coalesce
    // into a single network fetch. The first caller for `cache_key`
    // acquires the permit and runs steps 2-5; the rest park here
    // and, after acquiring, re-check the in-memory cache so the
    // winner's [`PackageMetaCache::set`] short-circuits them
    // without re-fetching. Without this, `try_join_all` over the
    // resolved tree fires N concurrent HTTP GETs per shared
    // packument (e.g. every `react-*` dep racing for `react`), each
    // queued behind the [`ThrottledClient`] semaphore — the
    // 3-5× resolve-walk gap the
    // [`alotta-files` benchmark]([../../../../../pnpm.io/benchmarks/results/pnpm12])
    // surfaced.
    let limit = {
        let entry = ctx
            .fetch_locker
            .entry(cache_key.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(1)));
        Arc::clone(entry.value())
    };
    let _permit = limit.acquire().await.expect("packument fetch semaphore should not be closed");

    // Re-check in-memory cache after acquiring the permit — the
    // previous permit holder may have just populated it. Without
    // this re-check, every duplicate caller would still fall
    // through to the disk + network path even though they were
    // waiting precisely for the winner's fetch to complete.
    if let Some(cached) = ctx.meta_cache.get(&cache_key) {
        return handle_cache_hit(
            ctx,
            spec,
            opts,
            &picker_opts,
            full_metadata,
            &cache_key,
            pkg_mirror.as_deref(),
            cached,
        )
        .await;
    }

    let mut meta_cached_in_store: Option<Arc<Package>> = None;

    // 2. Offline / pickLowestVersion / preferOffline disk read.
    if ctx.offline || ctx.prefer_offline || opts.pick_lowest_version {
        meta_cached_in_store = load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new);

        if ctx.offline {
            if let Some(meta) = meta_cached_in_store {
                let picked = pick_matching_version_final(&picker_opts, spec, &meta)?;
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
                meta,
            )
            .await?;
            let meta = upgrade.meta;
            if upgrade.upgraded && !opts.dry_run {
                if let Some(path) = pkg_mirror.as_deref() {
                    persist_upgraded_to_mirror(path, &meta);
                }
                ctx.meta_cache.set(cache_key.clone(), Arc::clone(&meta));
            }
            let picked = pick_matching_version_final(&picker_opts, spec, &meta)?;
            if picked.is_some() {
                return Ok(PickPackageResult { meta, picked_package: picked });
            }
            // Fall through to fetch when disk had the meta but no
            // version satisfied the spec — the disk copy may be
            // stale. Restore the (possibly upgraded) meta for later
            // paths that reuse the in-store load.
            meta_cached_in_store = Some(meta);
        }
    }

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
            // network fetch, which (in upstream pnpm) would
            // upgrade abbreviated→full. Pacquet's fetcher is
            // always full so this branch shouldn't fire today,
            // but the swallow-and-fall-through matches upstream.
            if let Ok(Some(picked)) = pick_matching_version_fast(&picker_opts, spec, meta) {
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
                    ctx.meta_cache.set(cache_key.clone(), Arc::clone(meta));
                }
                return Ok(PickPackageResult {
                    meta: Arc::clone(meta),
                    picked_package: Some(picked),
                });
            }
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
            && let Ok(Some(picked)) = pick_matching_version_fast(&picker_opts, spec, meta)
        {
            // Same rationale as the version-spec fast path above —
            // promote the disk-loaded packument into the
            // install-scoped in-memory cache.
            if !opts.dry_run {
                ctx.meta_cache.set(cache_key.clone(), Arc::clone(meta));
            }
            return Ok(PickPackageResult { meta: Arc::clone(meta), picked_package: Some(picked) });
        }
    }

    // 5. Network fetch via the cached fetcher. The cached fetcher
    //    handles conditional headers + 200 cache write internally;
    //    on a 304 it re-reads the mirror body. The error path here
    //    mirrors upstream: if a fetch failure has a disk fallback
    //    we use it; otherwise the error propagates.
    let fetch_opts = FetchFullMetadataCachedOptions {
        registry: opts.registry,
        http_client: ctx.http_client,
        auth_headers: ctx.auth_headers,
        cache_dir: ctx.cache_dir,
        full_metadata,
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
            let disk_fallback = match meta_cached_in_store {
                Some(meta) => Some(meta),
                None => load_meta_async(pkg_mirror.as_deref()).await.map(Arc::new),
            };
            if let Some(disk) = disk_fallback {
                tracing::debug!(
                    target: "pacquet_resolving_npm_resolver::pick_package",
                    ?error,
                    pkg_name = %spec.name,
                    "metadata fetch failed; falling back to on-disk mirror",
                );
                let picked = pick_matching_version_final(&picker_opts, spec, &disk)?;
                return Ok(PickPackageResult { meta: disk, picked_package: picked });
            }
            return Err(error.into());
        }
    };

    // After a fresh fetch we may still need an upgrade: a 304 reused
    // an abbreviated mirror body, or a 200 returned abbreviated data
    // for a recently-modified package. Either way, if
    // `published_by` is active and `meta.time` is missing, re-fetch
    // full so the maturity check runs on real timestamps. Mirrors
    // upstream's
    // [post-304 upgrade](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L333-L347)
    // and inline upgrade at lines 364-400.
    let upgrade =
        maybe_upgrade_abbreviated_meta_for_release_age(ctx, spec, opts, full_metadata, meta)
            .await?;
    let meta = upgrade.meta;
    if upgrade.upgraded
        && !opts.dry_run
        && let Some(path) = pkg_mirror.as_deref()
    {
        persist_upgraded_to_mirror(path, &meta);
    }

    // Divergence from upstream worth flagging: pnpm's pickPackage
    // gates the on-disk save behind `!opts.dryRun`. Pacquet's
    // `fetch_full_metadata_cached` already wrote the response body
    // to the mirror by the time it returned, so `opts.dry_run` only
    // suppresses the in-memory cache write. A future
    // refactor that threads `dry_run` into the fetcher can restore
    // upstream's no-disk-side-effect dry-run.
    if !opts.dry_run {
        ctx.meta_cache.set(cache_key, Arc::clone(&meta));
    }
    let picked = pick_matching_version_final(&picker_opts, spec, &meta)?;
    Ok(PickPackageResult { meta, picked_package: picked })
}

/// Shared cache-hit path. Invoked once on the optimistic pre-permit
/// check and once after the per-key permit is acquired (the re-check
/// that lets duplicate concurrent callers short-circuit without
/// re-fetching). Extracting it keeps the two call sites identical so
/// the upgrade-and-persist side-effects can't drift.
///
/// The argument list is wide because the helper consumes everything
/// the per-call frame already computed (cache key, derived
/// `full_metadata`, pre-resolved mirror path, picker options).
/// Bundling these into a struct would just shuffle the same fields
/// into a wrapper without removing any work; allowing the lint is
/// the lower-noise option.
#[allow(
    clippy::too_many_arguments,
    reason = "bundling these independent inputs into a struct moves the fields into a wrapper without removing work"
)]
async fn handle_cache_hit<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    picker_opts: &PickerOpts<'_>,
    full_metadata: bool,
    cache_key: &str,
    pkg_mirror: Option<&Path>,
    cached: Arc<Package>,
) -> Result<PickPackageResult, PickPackageError> {
    let upgrade =
        maybe_upgrade_abbreviated_meta_for_release_age(ctx, spec, opts, full_metadata, cached)
            .await?;
    let meta = upgrade.meta;
    if upgrade.upgraded && !opts.dry_run {
        // Persist so a fresh process doesn't re-trigger the upgrade
        // fetch on its next install. Matches upstream's
        // [`persistUpgradedMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L507-L524).
        if let Some(path) = pkg_mirror {
            persist_upgraded_to_mirror(path, &meta);
        }
        ctx.meta_cache.set(cache_key.to_string(), Arc::clone(&meta));
    }
    let picked = pick_matching_version_final(picker_opts, spec, &meta)?;
    Ok(PickPackageResult { meta, picked_package: picked })
}

/// Internal mirror of upstream's
/// [`PickerOptions`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L75-L79).
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
///
/// Mirrors upstream's
/// [`pickMatchingVersionFast`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L138-L146).
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

/// Picker used at terminal return sites where there's no further
/// fall-through. When `ignore_missing_time_field` is on, a
/// [`PickPackageFromMetaError::MissingTime`] surfaces as a one-shot
/// warning and the picker retries without `publishedBy`. Mirrors
/// upstream's
/// [`pickMatchingVersionFinal`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L152-L170).
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
            warn_missing_time_once(&pkg_name);
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

/// `publishedBy` is active: try highest mature; if no mature
/// version satisfies, fall back to lowest (regardless of maturity)
/// so the orchestrator can report the violation inline and let the
/// install layer decide what to do. Mirrors upstream's
/// [`pickRespectingMinReleaseAge`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L111-L123).
fn pick_respecting_min_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    run_picker(picker_opts, spec, |target_spec| {
        let highest = pick_package_from_meta(
            pick_version_by_version_range,
            &meta_opts(picker_opts),
            meta,
            target_spec,
        )?;
        if highest.is_some() {
            return Ok(highest);
        }
        // Fall-back lowest pick drops `publishedBy` so the picker
        // can return *something* even if every version is past the
        // cutoff. The install layer reads the resulting pick's
        // publish timestamp and surfaces the violation through the
        // verifier.
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

/// `publishedBy` is off: respect `pickLowestVersion`. Mirrors
/// upstream's
/// [`pickIgnoringReleaseAge`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L126-L133).
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
/// `latest` tag and return the higher of the two. Matches upstream's
/// [`runPicker`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L83-L92).
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
    let latest_spec = RegistryPackageSpec {
        name: spec.name.clone(),
        fetch_spec: "latest".to_string(),
        spec_type: RegistryPackageSpecType::Tag,
        normalized_bare_specifier: spec.normalized_bare_specifier.clone(),
    };
    let latest = pick_one(&latest_spec)?;
    Ok(pick_max(current, latest))
}

/// Higher-version-wins between two optional picks. Treats `None`
/// as "no pick" so a single satisfying option wins by default.
/// Mirrors upstream's
/// [`pickMax`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L95-L102).
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

fn validate_package_name(pkg_name: &str) -> Result<(), PickPackageError> {
    // Mirrors upstream's
    // [`validatePackageName`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L678-L682):
    // a slash without a `@scope/` prefix is structurally invalid.
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

/// Bounded set of package names we've already warned about for the
/// missing-`time` field. Matches upstream's
/// [`warnedMissingTimeFor`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L593-L605)
/// — a Set capped at 1024 entries to keep long-lived processes
/// (daemons, store servers) from leaking memory through it.
///
/// `IndexSet` (not `Vec`) gives O(1) `contains` + cheap insertion-
/// ordered eviction via `shift_remove_index(0)`, matching upstream's
/// JS `Set` which iterates in insertion order.
const MAX_WARNED_MISSING_TIME: usize = 1024;
static WARNED_MISSING_TIME: std::sync::OnceLock<Mutex<indexmap::IndexSet<String>>> =
    std::sync::OnceLock::new();

fn warn_missing_time_once(pkg_name: &str) {
    let lock = WARNED_MISSING_TIME.get_or_init(|| Mutex::new(indexmap::IndexSet::new()));
    let mut warned = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if warned.contains(pkg_name) {
        return;
    }
    if warned.len() >= MAX_WARNED_MISSING_TIME {
        // IndexSet preserves insertion order; drop the oldest entry
        // (index 0) so the bound stays at MAX_WARNED_MISSING_TIME.
        warned.shift_remove_index(0);
    }
    warned.insert(pkg_name.to_string());
    tracing::warn!(
        target: "pacquet_resolving_npm_resolver::pick_package",
        pkg_name,
        r#"The metadata of {pkg_name} is missing the "time" field; skipping the minimumReleaseAge check for this package."#,
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
    #[diagnostic(code(pacquet_resolving_npm_resolver::pick_package::encode_path))]
    EncodePath {
        #[error(not(source))]
        error: String,
    },
    #[display("Failed to serialize mirror entry: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::pick_package::serialize))]
    Serialize {
        #[error(not(source))]
        error: String,
    },
    #[display("Failed to write mirror entry: {error}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::pick_package::write))]
    Write {
        #[error(not(source))]
        error: String,
    },
}

/// Shared-state helper that lets a long-running install build one
/// [`PackageMetaCache`] and pass it (by [`Arc`]) to every
/// `pick_package` call.
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
    /// Matches upstream's `upgradedFrom != null` branch.
    upgraded: bool,
}

/// Port of upstream's
/// [`maybeUpgradeAbbreviatedMetaForReleaseAge`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L450-L501).
///
/// When the resolver default-fetched abbreviated metadata but
/// `published_by` is active, the per-version `time` map is missing
/// so the maturity check would silently degrade to the warn-and-skip
/// fallback. This function detects that and re-fetches full metadata
/// when the package's top-level `modified` field shows it was
/// touched after the maturity cutoff. Returns the original meta
/// untouched in every other case.
///
/// The early returns are guard rails that mirror upstream verbatim:
///
/// - `ctx.offline`: no network allowed. Stick with what we have.
/// - `opts.published_by.is_none()`: maturity check disabled.
/// - `meta.time.is_some()`: already full metadata (or an
///   abbreviated response that happens to carry `time`). Nothing
///   to upgrade.
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
/// abbreviated mirror via [`persist_upgraded_to_mirror`] — same
/// shape as upstream's `persistUpgradedMeta`, which intentionally
/// updates the *abbreviated* cache file with full data so the next
/// install sees `time` populated and skips the upgrade fetch.
///
/// The upgrade fetch forwards `meta.etag` and `meta.modified` as
/// conditional headers. When the registry's full-form representation
/// hasn't changed it answers `304 Not Modified` and the abbreviated
/// meta is returned untouched — matches upstream's `notModified`
/// short-circuit at
/// [`pickPackage.ts#L488-L499`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L488-L499).
async fn maybe_upgrade_abbreviated_meta_for_release_age<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    full_metadata: bool,
    meta: Arc<Package>,
) -> Result<UpgradeOutcome, PickPackageError> {
    if ctx.offline || full_metadata {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    let Some(cutoff) = opts.published_by else {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    };
    if meta.time.is_some() {
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
        // signal we have. Keep it and let the downstream picker
        // fall through to its warn-and-skip path on the missing
        // `time` map — mirrors upstream's `notModified` arm.
        FetchFullMetadataOutcome::NotModified => Ok(UpgradeOutcome { meta, upgraded: false }),
    }
}

/// Write the upgraded full metadata back to `pkg_mirror` (which
/// points at the abbreviated cache because the picker is in
/// abbreviated mode). Fire-and-forget: a write failure logs at debug
/// and the install proceeds — the next install simply re-triggers
/// the upgrade fetch. Mirrors upstream's
/// [`persistUpgradedMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L507-L524).
fn persist_upgraded_to_mirror(pkg_mirror: &Path, meta: &Package) {
    if let Err(error) = save_meta_indexed(pkg_mirror, meta, meta.etag.as_deref()) {
        tracing::debug!(
            target: "pacquet_resolving_npm_resolver::pick_package",
            ?error,
            path = %pkg_mirror.display(),
            "could not write upgraded meta to mirror; skipping persist",
        );
    }
}

#[cfg(test)]
mod tests;
