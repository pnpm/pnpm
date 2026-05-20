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
//! Compared to upstream this port simplifies one axis: pacquet's
//! metadata fetcher always returns *full* metadata (the verifier
//! needs it for `time` and trust evidence). The upstream code paths
//! that upgrade an abbreviated cache entry to full mid-pick are
//! therefore dead in pacquet today — the picker still goes through
//! the same shape so adding an abbreviated fetcher later is a
//! drop-in. Notes on the abbreviated paths are inline at the
//! sites they would activate.
//!
//! Concurrency: upstream uses `p-limit(1)` keyed on the mirror path
//! to serialize disk operations. Pacquet relies on the atomic
//! rename in [`crate::mirror::save_meta`] for write safety, and on
//! [`std::sync::Mutex`]-guarded in-memory caches for reader
//! coordination. The per-mirror limiter is omitted; if a future
//! issue forces serialization (Windows file-lock contention, e.g.)
//! it would land here as a map of `tokio::sync::Mutex` values.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::version_policy::PackageVersionPolicy;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::{Package, PackageVersion};
use pacquet_resolving_resolver_base::VersionSelectors;

use crate::{
    FetchFullMetadataCachedOptions, FetchMetadataError, fetch_full_metadata_cached,
    mirror::{FULL_META_DIR, get_pkg_mirror_path, load_meta, prepare_json_for_disk, save_meta},
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
    /// Cloned snapshot of the cached packument for `key`, or `None`
    /// when the cache hasn't seen it.
    fn get(&self, key: &str) -> Option<Package>;
    /// Insert/overwrite `meta` under `key`. The orchestrator only
    /// inserts after a fresh fetch — never replays a stale on-disk
    /// load.
    fn set(&self, key: String, meta: Package);
}

/// Default thread-safe [`PackageMetaCache`] backed by a [`Mutex`]
/// guarding a [`HashMap`]. A consumer that already has its own
/// shared map can implement the trait directly instead of using
/// this.
#[derive(Debug, Default)]
pub struct InMemoryPackageMetaCache {
    inner: Mutex<HashMap<String, Package>>,
}

impl PackageMetaCache for InMemoryPackageMetaCache {
    fn get(&self, key: &str) -> Option<Package> {
        // Mirror the rest of the codebase (e.g. `build_modules.rs`):
        // recover from poisoning instead of escalating an unrelated
        // panic into a hard install-wide failure. The cache is a
        // plain HashMap of cloneable values — no broken invariants
        // can survive across a poisoned lock.
        self.inner.lock().unwrap_or_else(|err| err.into_inner()).get(key).cloned()
    }

    fn set(&self, key: String, meta: Package) {
        self.inner.lock().unwrap_or_else(|err| err.into_inner()).insert(key, meta);
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
    /// Pacquet's full-metadata fetcher always returns `time` when
    /// the registry exposes it, so the missing-time path here is
    /// only reachable when the registry itself stripped the field —
    /// rare, but the opt-in stays for parity with the resolver
    /// option flag.
    pub ignore_missing_time_field: bool,
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
}

/// Outcome of a successful [`pick_package`] call. Mirrors
/// upstream's `{ meta, pickedPackage }`.
#[derive(Debug)]
pub struct PickPackageResult {
    pub meta: Package,
    pub picked_package: Option<PackageVersion>,
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

    let pkg_mirror = ctx
        .cache_dir
        .and_then(|dir| get_pkg_mirror_path(dir, FULL_META_DIR, opts.registry, &spec.name).ok());

    // Scope the in-memory cache key by registry so the same package
    // name in two different registries (private + public, scoped
    // override, etc.) never short-circuits to the wrong packument.
    // Upstream pnpm gets the same scoping by holding one
    // `PackageMetaCache` per resolver instance per registry; pacquet
    // shares one cache across all `pick_package` calls, so the key
    // has to do the scoping itself.
    let cache_key = format!("{}\x00{}", opts.registry, spec.name);

    // 1. In-memory cache.
    if let Some(cached) = ctx.meta_cache.get(&cache_key) {
        let picked = pick_matching_version_final(&picker_opts, spec, &cached)?;
        return Ok(PickPackageResult { meta: cached, picked_package: picked });
    }

    let mut meta_cached_in_store: Option<Package> = None;

    // 2. Offline / pickLowestVersion / preferOffline disk read.
    if ctx.offline || ctx.prefer_offline || opts.pick_lowest_version {
        meta_cached_in_store = pkg_mirror.as_deref().and_then(load_meta);

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

        if let Some(ref meta) = meta_cached_in_store {
            let picked = pick_matching_version_final(&picker_opts, spec, meta)?;
            if picked.is_some() {
                return Ok(PickPackageResult { meta: meta.clone(), picked_package: picked });
            }
            // Fall through to fetch when disk had the meta but no
            // version satisfied the spec — the disk copy may be
            // stale.
        }
    }

    // 3. Version-spec fast path.
    if !opts.include_latest_tag && matches!(spec.spec_type, RegistryPackageSpecType::Version) {
        if meta_cached_in_store.is_none() {
            meta_cached_in_store = pkg_mirror.as_deref().and_then(load_meta);
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
                return Ok(PickPackageResult { meta: meta.clone(), picked_package: Some(picked) });
            }
        }
    }

    // 4. publishedBy mtime shortcut.
    if let Some(published_by) = opts.published_by
        && let Some(mtime) = pkg_mirror.as_deref().and_then(get_file_mtime)
        && mtime >= published_by
    {
        if meta_cached_in_store.is_none() {
            meta_cached_in_store = pkg_mirror.as_deref().and_then(load_meta);
        }
        if let Some(ref meta) = meta_cached_in_store
            && let Ok(Some(picked)) = pick_matching_version_fast(&picker_opts, spec, meta)
        {
            return Ok(PickPackageResult { meta: meta.clone(), picked_package: Some(picked) });
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
    };

    let fetch_result = fetch_full_metadata_cached(&spec.name, &fetch_opts).await;
    let meta = match fetch_result {
        Ok(meta) => meta,
        Err(error) => {
            // The fetcher already saved a 200 to disk before it
            // returned (when it returned Ok). If it returned Err,
            // try the disk fallback: an existing mirror is good
            // enough to pick from, even if the latest sync failed.
            if let Some(disk) =
                meta_cached_in_store.or_else(|| pkg_mirror.as_deref().and_then(load_meta))
            {
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

    // Divergence from upstream worth flagging: pnpm's pickPackage
    // gates the on-disk save behind `!opts.dryRun`. Pacquet's
    // `fetch_full_metadata_cached` already wrote the response body
    // to the mirror by the time it returned, so `opts.dry_run` only
    // suppresses the in-memory cache write. A future
    // refactor that threads `dry_run` into the fetcher can restore
    // upstream's no-disk-side-effect dry-run.
    if !opts.dry_run {
        ctx.meta_cache.set(cache_key, meta.clone());
    }
    let picked = pick_matching_version_final(&picker_opts, spec, &meta)?;
    Ok(PickPackageResult { meta, picked_package: picked })
}

/// Internal mirror of upstream's
/// [`PickerOptions`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L75-L79).
/// Same fields as [`PickPackageOptions`] minus the dispatcher-only
/// ones (registry, dry_run); plus the `ignore_missing_time_field`
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
) -> Result<Option<PackageVersion>, PickPackageFromMetaError> {
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
) -> Result<Option<PackageVersion>, PickPackageFromMetaError> {
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
) -> Result<Option<PackageVersion>, PickPackageFromMetaError> {
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
) -> Result<Option<PackageVersion>, PickPackageFromMetaError> {
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
) -> Result<Option<PackageVersion>, PickPackageFromMetaError>
where
    PickOne: Fn(&RegistryPackageSpec) -> Result<Option<PackageVersion>, PickPackageFromMetaError>,
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
fn pick_max(lhs: Option<PackageVersion>, rhs: Option<PackageVersion>) -> Option<PackageVersion> {
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
    let mut warned = lock.lock().unwrap_or_else(|err| err.into_inner());
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
/// `<cache_dir>/<FULL_META_DIR>/<registry>/<encoded-pkg>.jsonl`.
/// Errors are logged at debug — a cache-write failure should never
/// fail an install. Kept public so the rare caller that
/// constructs a `Package` outside the fetcher (test fixtures, the
/// integrated benchmark's pre-warmer) can seed the mirror without
/// reaching into `crate::mirror`.
pub fn persist_meta_to_mirror(
    cache_dir: &Path,
    registry: &str,
    meta: &Package,
) -> Result<(), MirrorPersistError> {
    let path = get_pkg_mirror_path(cache_dir, FULL_META_DIR, registry, &meta.name)
        .map_err(|error| MirrorPersistError::EncodePath { error: error.to_string() })?;
    let json = prepare_json_for_disk(meta, meta.etag.as_deref(), None)
        .map_err(|error| MirrorPersistError::Serialize { error: error.to_string() })?;
    save_meta(&path, &json).map_err(|error| MirrorPersistError::Write { error: error.to_string() })
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
pub fn shared_in_memory_cache() -> Arc<InMemoryPackageMetaCache> {
    Arc::new(InMemoryPackageMetaCache::default())
}

#[cfg(test)]
mod tests;
