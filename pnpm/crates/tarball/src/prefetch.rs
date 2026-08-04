//! Bulk store-index prefetch.
//!
//! Reads every already-present package's CAS row in one pass before
//! the install fans out, so the warm batch can skip per-package
//! store-index lookups entirely.

use super::{
    Arc, HashMap, IntoParallelIterator, PackageFilesIndex, ParallelIterator, PathBuf,
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, files_include_install_scripts,
    manifest_requires_build,
};

/// Try to reconstruct the `{filename → CAFS path}` map for a package from
/// the `SQLite` store index, without going to the network. Returns `None`
/// if anything looks off — no index handed in, no row, unreadable row,
/// failed integrity check — so the caller falls through to a fresh
/// download.
///
/// The `verify_store_integrity` parameter matches pnpm's flag of the
/// same name. When `true` (pnpm's default) each referenced CAFS file is
/// stat'ed and compared against the stored `checkedAt`/size, with a
/// re-hash only when the mtime has advanced. When `false` the lookup
/// builds the filename→path map straight from the index row without any
/// filesystem work — missing / corrupt CAFS blobs surface lazily when
/// the caller tries to import them.
///
/// Corruption is caught via the content hash, not by gating on the
/// dirent type.
///
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

/// `requiresBuild` flags recovered from the same `index.db` rows as
/// [`PrefetchedCasPaths`]. Missing values in old rows are recomputed
/// from the bundled manifest plus verified file keys, mirroring
/// pnpm's worker fallback when `pkgFilesIndex.requiresBuild` is absent.
pub type PrefetchedRequiresBuild = HashMap<String, bool>;

pub(crate) type DecodedPrefetchRow =
    (String, Option<Arc<serde_json::Value>>, Option<bool>, pacquet_store_dir::VerifyResult);

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
    pub requires_build: PrefetchedRequiresBuild,
}

/// Batch the entire warm-cache lookup phase into one `spawn_blocking`
/// task at install start: collect every row the lockfile is going to
/// ask about under a single `index.lock()` round-trip, drop the lock,
/// then run the per-package integrity checks unlocked. Returns a
/// `cache_key → Arc<cas_paths>` map the per-snapshot futures can hit
/// synchronously.
///
/// **Locking shape (see [#292]):** the `SQLite` mutex
/// is held only for the SELECT loop. Integrity checks (`fs::metadata`
/// per file, optional re-hash) happen after the guard drops, so a
/// concurrent reader on the same `SharedReadonlyStoreIndex` doesn't
/// have to wait through the whole batch's filesystem work.
///
/// **Why one batched task instead of 1352 `spawn_blockings`:** the
/// per-snapshot path fans out one `tokio::task::spawn_blocking` per
/// snapshot. With 1352 snapshots all firing into the default
/// 512-thread blocking pool, threads compete for CPU and get
/// preempted between fs ops — sample-profiling showed cache-lookup
/// bodies averaging 20-60 ms each (sum 26-82 s) almost entirely
/// blocked, even though the actual SELECT (≈40 µs) and per-file
/// integrity stats (≈ms each) shouldn't take that long. Doing the
/// whole batch on one thread avoids the OS-scheduler / kernel-journal
/// thrash and makes each query fast in CPU-time. Pnpm's piscina pool
/// achieves the same shape implicitly with 4 dedicated workers.
///
/// Cache misses (no row, malformed row, integrity-check failure)
/// just don't appear in the result. The caller then falls through
/// to [`crate::download::DownloadTarballToStore::run_without_mem_cache`] for those
/// keys, which still has its own cache check as a backstop.
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
        // Phase 1: read every row's *raw bytes* under the mutex.
        // Splitting raw-read from decode means the
        // `SharedReadonlyStoreIndex` lock is held only for the
        // SELECT loop, not for the per-row msgpackr decode — which
        // is the dominant CPU cost once rows carry a `manifest`
        // field (transcode + `rmp_serde::from_slice` of a nested
        // JSON tree per row, times ~1k rows on a real lockfile).
        // Doing the decode after the guard drops lets it fan out
        // across rayon below.
        //
        // One batched `SELECT ... WHERE key IN (?, ?, ...)` per
        // `GET_MANY_CHUNK` (see `StoreIndex::get_many_raw`)
        // collapses what used to be N round-trips into one — see
        // <https://github.com/pnpm/pacquet/issues/294> for the cold-cache regression the per-key loop
        // introduced when every key missed.
        let raw: Vec<(String, Vec<u8>)> = {
            let Ok(guard) = index.lock() else {
                tracing::debug!(
                    target: "pacquet::download",
                    "store-index mutex poisoned at prefetch start; falling back to per-snapshot lookups",
                );
                return PrefetchResult::default();
            };
            match guard.get_many_raw(&cache_keys) {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::debug!(
                        target: "pacquet::download",
                        ?error,
                        "store-index batched read failed at prefetch start; falling back to per-snapshot lookups",
                    );
                    return PrefetchResult::default();
                }
            }
        };
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
                let mut entry: PackageFilesIndex = match pacquet_store_dir::decode_package_files_index(&bytes) {
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
                let stored_requires_build = entry.requires_build;
                let manifest = entry.manifest.take().map(Arc::new);
                let verify_result = if verify_store_integrity {
                    pacquet_store_dir::check_pkg_files_integrity(
                        store_dir,
                        entry,
                        &verified_files_cache,
                    )
                } else {
                    pacquet_store_dir::build_file_maps_from_index(store_dir, entry)
                };
                Some((cache_key, manifest, stored_requires_build, verify_result))
            })
            .collect();

        let mut cas_paths = HashMap::with_capacity(decoded.len());
        let mut manifests = HashMap::new();
        let mut side_effects_maps = HashMap::new();
        let mut requires_build = HashMap::with_capacity(decoded.len());
        for (cache_key, manifest, stored_requires_build, verify_result) in decoded {
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
                requires_build.insert(cache_key.clone(), calculated_requires_build);
                cas_paths.insert(cache_key, Arc::new(verify_result.files_map));
            }
        }
        PrefetchResult { cas_paths, manifests, side_effects_maps, requires_build }
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

/// The `index` argument is a shared read-only handle that callers open
/// once per install and pass in repeatedly, so we don't pay the
/// `Connection::open` + PRAGMA cost per package.
pub(crate) async fn load_cached_cas_paths(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    cache_key: String,
    verify_store_integrity: bool,
    verified_files_cache: SharedVerifiedFilesCache,
) -> Option<HashMap<String, PathBuf>> {
    let index = index?;
    // Hold on to a copy of the cache key for the outer `JoinError` log,
    // since the task body moves the original in.
    let outer_cache_key = cache_key.clone();
    let result = tokio::task::spawn_blocking(move || -> Option<HashMap<String, PathBuf>> {
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
                return None;
            };
            guard.get(&cache_key).ok()?
        }?;

        let verify_result = if verify_store_integrity {
            pacquet_store_dir::check_pkg_files_integrity(store_dir, entry, &verified_files_cache)
        } else {
            pacquet_store_dir::build_file_maps_from_index(store_dir, entry)
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
            return None;
        }
        Some(verify_result.files_map)
    })
    .await;

    match result {
        Ok(cas_paths) => cas_paths,
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
            None
        }
    }
}
