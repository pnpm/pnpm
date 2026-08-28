//! Bulk store-index prefetch.
//!
//! Reads every already-present package's CAS row in one pass before
//! the install fans out, so the warm batch can skip per-package
//! store-index lookups entirely.

use super::{Arc, HashMap, IntoParallelIterator, ParallelIterator, PathBuf, TarballError};
use pnpm_package_manifest::{files_include_install_scripts, manifest_requires_build};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use pnpm_store_dir::{
    PackageFilesIndex, PkgContentMismatch, SharedReadonlyStoreIndex, SharedVerifiedFilesCache,
    StoreDir,
};

/// Pre-fetched cas-paths map shared across all per-snapshot futures.
/// Built once at install start by [`prefetch_cas_paths`]; downloads
/// consult it before falling through to a per-snapshot `SQLite` lookup.
///
/// Values are `Arc`-wrapped so the cold-batch fallback can hand a hit
/// back as a cheap pointer-clone rather than memcpy-ing the whole
/// per-file map (each entry is a `HashMap<String, PathBuf>` with up
/// to ~hundred entries, so the deep clone is a hot-path cost).
pub type PrefetchedCasPaths = HashMap<String, Arc<HashMap<String, PathBuf>>>;

/// Bundled package manifests recovered from the `SQLite` store index,
/// keyed by the same `<integrity>\t<pkg_id>` string [`PrefetchedCasPaths`]
/// uses. The parsed manifest is read out of the row's `manifest` field so
/// bin linking doesn't have to re-read `package.json` from disk per child.
/// Each value is `Arc`-wrapped so multiple bin-link consumers can hold the
/// same parsed manifest without deep-cloning.
///
/// Only keys whose row carried a manifest blob appear in the map —
/// a missing key means either "row exists but has no manifest" (old
/// pacquet write, or a tarball whose `package.json` failed to
/// parse) or "package wasn't prefetched at all". Callers that need
/// to tell those apart cross-reference with [`PrefetchedCasPaths`]
/// from the same [`PrefetchResult`].
pub type PrefetchedManifests = HashMap<String, Arc<serde_json::Value>>;

/// Side-effects-cache overlays recovered from the same `index.db`
/// rows as [`PrefetchedCasPaths`]. The outer key is the same
/// `<integrity>\t<pkg_id>` store-index row key; the inner map is
/// the per-row `cache_key → FilesMap` table that `VerifyResult`
/// produces (already with the `added` / `deleted` overlay applied
/// against the base files). Carries the same per-package side-effects
/// maps pnpm threads through its package-files response.
///
/// Pacquet hands these off to `BuildModules`'s `is_built` gate —
/// the build-phase skips a snapshot when its computed
/// `calc_dep_state` cache key has a matching entry here.
///
/// Outer values are `Arc`-wrapped for the same cold-batch cheap-clone
/// reason [`PrefetchedCasPaths`] is.
pub type PrefetchedSideEffectsMaps =
    HashMap<String, Arc<HashMap<String, HashMap<String, PathBuf>>>>;

pub type PrefetchedSideEffects =
    HashMap<String, Arc<HashMap<String, pnpm_store_dir::SideEffectsDiff>>>;

pub type PrefetchedRemoteSideEffectsQuarantine = HashMap<String, Arc<HashMap<String, Vec<String>>>>;

/// `requiresBuild` flags recovered from the same `index.db` rows as
/// [`PrefetchedCasPaths`]. Missing values in old rows are recomputed
/// from the bundled manifest plus verified file keys, mirroring
/// pnpm's worker fallback when `pkgFilesIndex.requiresBuild` is absent.
pub type PrefetchedRequiresBuild = HashMap<String, bool>;

/// `requiresPrepare` flags present in git package store-index rows.
pub type PrefetchedRequiresPrepare = HashMap<String, bool>;

pub(crate) type DecodedPrefetchRow = (
    String,
    Option<Arc<serde_json::Value>>,
    Option<bool>,
    Option<bool>,
    pnpm_store_dir::VerifyResult,
);

/// Output of [`prefetch_cas_paths`]: the warm-cache filesystem map
/// plus any bundled manifests and side-effects overlays recovered
/// from the same `index.db` rows. Bundled in a single struct so
/// callers can destructure all cached facts after one `await`, rather
/// than the function having to thread several separate `spawn_blocking`
/// round-trips through.
#[derive(Default)]
pub struct PrefetchResult {
    pub cas_paths: PrefetchedCasPaths,
    pub manifests: PrefetchedManifests,
    pub side_effects_maps: PrefetchedSideEffectsMaps,
    pub side_effects: PrefetchedSideEffects,
    pub remote_side_effects_quarantine: PrefetchedRemoteSideEffectsQuarantine,
    pub requires_build: PrefetchedRequiresBuild,
    pub requires_prepare: PrefetchedRequiresPrepare,
}

/// Resolve the whole install's warm-cache lookups up front, returning a
/// `cache_key → Arc<cas_paths>` map the per-snapshot futures hit
/// synchronously. Keys with no row, an undecodable row, or a failed
/// integrity check are absent, and fall through to their per-snapshot
/// lookup.
///
/// Runs as one `spawn_blocking` rather than one per snapshot: at ~1.3k
/// snapshots the default 512-thread blocking pool spends its time
/// descheduling rather than working, and profiling put lookup bodies at
/// 20-60 ms apiece against a ≈40 µs query. See [#292].
///
/// [#292]: https://github.com/pnpm/pacquet/pull/292
pub async fn prefetch_cas_paths(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    cache_keys: Vec<String>,
    verify_store_integrity: bool,
    verified_files_cache: SharedVerifiedFilesCache,
) -> PrefetchResult {
    let Some(index) = index else { return PrefetchResult::default() };
    if cache_keys.is_empty() {
        return PrefetchResult::default();
    }
    let result = tokio::task::spawn_blocking(move || -> PrefetchResult {
        let read_start = std::time::Instant::now();
        let Some(raw) = read_raw_rows_under_lock(&index, &cache_keys) else {
            return PrefetchResult::default();
        };
        let read_ms = read_start.elapsed().as_millis() as u64;
        let decode_start = std::time::Instant::now();
        // Phase 2: decode each row's msgpackr-records bytes into a
        // `PackageFilesIndex`, then run the integrity check. Both
        // steps are per-row CPU work with no shared state, so we
        // fan out across rayon. With manifests included in the
        // payload, decoding 1k+ rows serially had become the
        // dominant chunk of the prefetch wall (single-threaded
        // `spawn_blocking`); the par-iter recovers the per-row
        // parallelism the warm-batch link phase already uses.
        //
        // The bundled manifest is split off the decoded entry via
        // `Option::take` so it travels back to the caller without
        // an intermediate `Value::clone` of the JSON tree — the
        // verify function only inspects `files`, never `manifest`.
        let decoded: Vec<DecodedPrefetchRow> = raw
            .into_par_iter()
            .filter_map(|(cache_key, bytes)| {
                let mut entry: PackageFilesIndex =
                    match pnpm_store_dir::decode_package_files_index(&bytes) {
                        Ok(entry) => entry,
                        Err(error) => {
                            tracing::debug!(
                                target: "pacquet::download",
                                ?cache_key,
                                ?error,
                                "skipping undecodable package_index row at prefetch",
                            );
                            return None;
                        }
                    };
                if let Some(mismatch) =
                    pnpm_store_dir::pkg_content_mismatch(entry.manifest.as_ref(), &cache_key)
                {
                    // Left to the per-snapshot lookup, which is the one
                    // place that reports the disagreement — as an error
                    // under `strictStorePkgContentCheck`, as a warning
                    // without it. Skipping here costs a re-read of the
                    // row in the latter case and nothing in the former.
                    tracing::debug!(
                        target: "pacquet::download",
                        ?cache_key,
                        expected = mismatch.expected,
                        actual = mismatch.actual,
                        "store-index row holds another package; leaving it to the per-snapshot lookup",
                    );
                    return None;
                }
                let stored_requires_build = entry.requires_build;
                let stored_requires_prepare = entry.requires_prepare;
                let manifest = entry.manifest.take().map(Arc::new);
                let verify_result = if verify_store_integrity {
                    pnpm_store_dir::check_pkg_files_integrity(
                        store_dir,
                        entry,
                        &verified_files_cache,
                    )
                } else {
                    pnpm_store_dir::build_file_maps_from_index(store_dir, entry)
                };
                Some((
                    cache_key,
                    manifest,
                    stored_requires_build,
                    stored_requires_prepare,
                    verify_result,
                ))
            })
            .collect();
        tracing::debug!(
            target: "pacquet::download",
            rows = decoded.len(),
            read_ms,
            decode_verify_ms = decode_start.elapsed().as_millis() as u64,
            "prefetch timings",
        );

        let mut cas_paths = HashMap::with_capacity(decoded.len());
        let mut manifests = HashMap::new();
        let mut side_effects_maps = HashMap::new();
        let mut side_effects = HashMap::new();
        let mut remote_side_effects_quarantine = HashMap::new();
        let mut requires_build = HashMap::with_capacity(decoded.len());
        let mut requires_prepare = HashMap::new();
        for (cache_key, manifest, stored_requires_build, stored_requires_prepare, verify_result) in
            decoded
        {
            if verify_result.passed {
                let calculated_requires_build = stored_requires_build.unwrap_or_else(|| {
                    manifest.as_deref().is_some_and(manifest_requires_build)
                        || files_include_install_scripts(verify_result.files_map.keys())
                });
                if let Some(manifest) = manifest {
                    manifests.insert(cache_key.clone(), manifest);
                }
                if let Some(maps) = verify_result.side_effects_maps
                    && !maps.is_empty()
                {
                    side_effects_maps.insert(cache_key.clone(), Arc::new(maps));
                }
                if let Some(diffs) = verify_result.side_effects
                    && !diffs.is_empty()
                {
                    side_effects.insert(cache_key.clone(), Arc::new(diffs));
                }
                if let Some(quarantine) = verify_result.remote_side_effects_quarantine
                    && !quarantine.is_empty()
                {
                    remote_side_effects_quarantine
                        .insert(cache_key.clone(), Arc::new(quarantine));
                }
                requires_build.insert(cache_key.clone(), calculated_requires_build);
                if let Some(requires_prepare_value) = stored_requires_prepare {
                    requires_prepare.insert(cache_key.clone(), requires_prepare_value);
                }
                cas_paths.insert(cache_key, Arc::new(verify_result.files_map));
            }
        }
        PrefetchResult {
            cas_paths,
            manifests,
            side_effects_maps,
            side_effects,
            remote_side_effects_quarantine,
            requires_build,
            requires_prepare,
        }
    })
    .await;
    result.unwrap_or_else(|error| {
        tracing::warn!(
            target: "pacquet::download",
            ?error,
            "store-index prefetch task failed; falling back to per-snapshot lookups",
        );
        PrefetchResult::default()
    })
}

/// What the store-index lookup found for one row.
enum CachedRow {
    /// No row, an unreadable one, or one whose files no longer verify.
    Miss,
    /// A row holding another package, under a strict content check: not
    /// read any further.
    Rejected(PkgContentMismatch),
    /// A usable row's per-file CAS map, carrying the identity
    /// disagreement the caller warns about when the check is not strict.
    Hit { cas_paths: HashMap<String, PathBuf>, mismatch: Option<PkgContentMismatch> },
}

/// Reconstruct a package's `{filename → CAFS path}` map from the
/// `SQLite` store index instead of the network. `Ok(None)` on any doubt
/// — no index, no row, unreadable row, failed integrity check — leaving
/// the caller to download.
///
/// `verify_store_integrity` matches pnpm's flag of the same name, and
/// what it buys is narrower than the name suggests: a file whose mtime
/// has not advanced past the recorded `checkedAt` is accepted on the
/// stat alone, so it catches decay rather than tampering that preserves
/// the timestamp. With it off, a missing or corrupt blob surfaces later,
/// when the caller tries to import it.
///
/// `strict_store_pkg_content_check` is pnpm's `strictStorePkgContentCheck`:
/// a row whose bundled manifest names another package fails the install
/// under it, and is used with a warning without it. Either way the row
/// is never silently swapped for a download — that would hide a broken
/// lockfile behind a slow install.
///
/// `index` is opened once per install and passed in repeatedly, so the
/// `Connection::open` + PRAGMA cost is not paid per package.
pub(crate) async fn load_cached_cas_paths<Reporter: crate::Reporter>(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    cache_key: String,
    verify_store_integrity: bool,
    strict_store_pkg_content_check: bool,
    verified_files_cache: SharedVerifiedFilesCache,
) -> Result<Option<HashMap<String, PathBuf>>, TarballError> {
    let Some(index) = index else { return Ok(None) };
    // Hold on to a copy of the cache key for the outer `JoinError` log,
    // since the task body moves the original in.
    let outer_cache_key = cache_key.clone();
    let result = tokio::task::spawn_blocking(move || -> CachedRow {
        // Treat a poisoned mutex as a cache miss rather than propagating the
        // panic: the `SELECT` is stateless, so the prior panic couldn't have
        // left the index in an inconsistent shape, and cache lookups are a
        // best-effort hint anyway — failing over to a fresh download is the
        // more resilient default than turning every subsequent snapshot into
        // a crash.
        let entry = {
            let Ok(guard) = index.lock() else {
                tracing::debug!(
                    target: "pacquet::download",
                    ?cache_key,
                    "store-index mutex poisoned; treating cache lookup as a miss",
                );
                return CachedRow::Miss;
            };
            match guard.get(&cache_key) {
                Ok(Some(entry)) => entry,
                Ok(None) | Err(_) => return CachedRow::Miss,
            }
        };

        let mismatch = pnpm_store_dir::pkg_content_mismatch(entry.manifest.as_ref(), &cache_key);
        if strict_store_pkg_content_check && let Some(mismatch) = mismatch {
            return CachedRow::Rejected(mismatch);
        }

        let verify_result = if verify_store_integrity {
            pnpm_store_dir::check_pkg_files_integrity(store_dir, entry, &verified_files_cache)
        } else {
            pnpm_store_dir::build_file_maps_from_index(store_dir, entry)
        };
        if !verify_result.passed {
            // Per-file reason (filename, CAS path, size mismatch, hash
            // mismatch, ...) is logged at `debug!` inside
            // `check_pkg_files_integrity` / `build_file_maps_from_index`
            // where the failure actually happens — this caller-side log
            // just summarises "the row as a whole didn't verify" so log
            // scrapers can correlate the per-file debug lines with the
            // snapshot they belong to.
            tracing::debug!(
                target: "pacquet::download",
                ?cache_key,
                "store-index entry failed integrity check; re-fetching",
            );
            return CachedRow::Miss;
        }
        CachedRow::Hit { cas_paths: verify_result.files_map, mismatch }
    })
    .await;

    match result {
        Ok(CachedRow::Miss) => Ok(None),
        Ok(CachedRow::Rejected(mismatch)) => {
            Err(TarballError::UnexpectedPkgContentInStore { hint: mismatch.hint() })
        }
        Ok(CachedRow::Hit { cas_paths, mismatch }) => {
            if let Some(mismatch) = mismatch {
                Reporter::emit(&LogEvent::Global(GlobalLog {
                    level: LogLevel::Warn,
                    message: format!(
                        "Package name or version mismatch found while reading from the store. {}",
                        mismatch.hint(),
                    ),
                }));
            }
            Ok(Some(cas_paths))
        }
        Err(error) => {
            // `JoinError` — the blocking task panicked, or the runtime was
            // cancelled mid-install. Degrade to a cache miss so the caller
            // falls through to a fresh download, but surface the error so
            // the panic / cancellation stays diagnosable.
            tracing::warn!(
                target: "pacquet::download",
                ?error,
                cache_key = ?outer_cache_key,
                "store-index lookup task failed; treating cache lookup as a miss",
            );
            Ok(None)
        }
    }
}

/// Read every requested row's undecoded bytes, holding the store-index
/// mutex for the `SELECT` loop alone.
///
/// Decoding is the dominant cost once rows carry a `manifest` — a
/// nested JSON tree per row, across ~1k rows on a real lockfile — so it
/// stays outside the guard, leaving concurrent readers to wait only on
/// the queries. `get_many_raw` batches those into one round-trip per
/// `GET_MANY_CHUNK` rather than one per key, which is what makes a
/// cold cache affordable: <https://github.com/pnpm/pacquet/issues/294>.
///
/// `None` means the prefetch cannot proceed and every key should fall
/// through to its per-snapshot lookup.
fn read_raw_rows_under_lock(
    index: &SharedReadonlyStoreIndex,
    cache_keys: &[String],
) -> Option<Vec<(String, Vec<u8>)>> {
    let Ok(guard) = index.lock() else {
        tracing::debug!(
            target: "pacquet::download",
            "store-index mutex poisoned at prefetch start; falling back to per-snapshot lookups",
        );
        return None;
    };
    guard
        .get_many_raw(cache_keys)
        .inspect_err(|error| {
            tracing::debug!(
                target: "pacquet::download",
                ?error,
                "store-index batched read failed at prefetch start; falling back to per-snapshot lookups",
            );
        })
        .ok()
}
