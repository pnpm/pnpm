//! Bulk store-index prefetch.
//!
//! Reads every already-present package's CAS row in one pass before
//! the install fans out, so the warm batch can skip per-package
//! store-index lookups entirely.

use super::{
    Arc, ArchiveStoreProjection, HashMap, IntoParallelIterator, PackageContentCheck,
    ParallelIterator, PathBuf, TarballError,
};
use pnpm_package_manifest::{files_include_install_scripts, manifest_requires_build};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use pnpm_store_dir::{
    PackageFilesIndex, PendingFilesCheck, PkgContentMismatch, SharedReadonlyStoreIndex,
    SharedVerifiedFilesCache, StoreDir,
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

pub(crate) struct DecodedPrefetchRow {
    cache_key: String,
    manifest: Option<Arc<serde_json::Value>>,
    stored_requires_build: Option<bool>,
    stored_requires_prepare: Option<bool>,
    verify_result: pnpm_store_dir::VerifyResult,
    /// The row's files check when it was deferred — see
    /// [`PrefetchIntegrityCheck::Deferred`].
    pending_check: Option<PendingFilesCheck>,
}

/// When [`prefetch_cas_paths`] checks a row's CAFS files against the
/// digests the row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchIntegrityCheck {
    /// Check every row as it is read. A row that fails is left out of
    /// the result.
    Eager,
    /// Read every row trusting its files, and check a row only when the
    /// caller names it in [`PrefetchResult::verify_rows`]. For a caller
    /// that reads the whole lockfile but materializes a few snapshots,
    /// this keeps the per-file stats to the rows it imports.
    Deferred,
    /// `verifyStoreIntegrity: false`. Rows are trusted; a missing or
    /// corrupt CAFS file surfaces at import time.
    Skip,
}

impl PrefetchIntegrityCheck {
    /// [`Self::Eager`] under `verifyStoreIntegrity`, else [`Self::Skip`].
    #[must_use]
    pub fn eager_if(verify_store_integrity: bool) -> Self {
        if verify_store_integrity { Self::Eager } else { Self::Skip }
    }

    /// [`Self::Deferred`] under `verifyStoreIntegrity`, else [`Self::Skip`].
    #[must_use]
    pub fn deferred_if(verify_store_integrity: bool) -> Self {
        if verify_store_integrity { Self::Deferred } else { Self::Skip }
    }
}

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
    /// The files checks [`PrefetchIntegrityCheck::Deferred`] left to
    /// [`Self::verify_rows`], by store-index row key. Empty under the
    /// other modes.
    pub pending_checks: HashMap<String, PendingFilesCheck>,
}

impl PrefetchResult {
    /// Run the deferred files checks of the named rows, dropping every
    /// row that fails from all the maps so its snapshot falls through
    /// to the per-snapshot lookup and re-fetch. A key that has no
    /// pending check (already verified, never prefetched, or checked
    /// by an earlier call) is skipped. Returns the number of rows
    /// dropped.
    ///
    /// Blocking: the checks stat every file of every named row, fanned
    /// out across rayon.
    pub fn verify_rows<'a>(
        &mut self,
        cache_keys: impl IntoIterator<Item = &'a str>,
        store_dir: &StoreDir,
        verified_files_cache: &SharedVerifiedFilesCache,
    ) -> usize {
        let checks: Vec<(String, PendingFilesCheck)> = cache_keys
            .into_iter()
            .filter_map(|cache_key| self.pending_checks.remove_entry(cache_key))
            .collect();
        let failed: Vec<String> = checks
            .into_par_iter()
            .filter_map(|(cache_key, check)| {
                (!check.verify(store_dir, verified_files_cache)).then_some(cache_key)
            })
            .collect();
        for cache_key in &failed {
            tracing::debug!(
                target: "pacquet::download",
                ?cache_key,
                "store-index entry failed integrity check; leaving it to the per-snapshot lookup",
            );
            self.cas_paths.remove(cache_key);
            self.manifests.remove(cache_key);
            self.side_effects_maps.remove(cache_key);
            self.side_effects.remove(cache_key);
            self.remote_side_effects_quarantine.remove(cache_key);
            self.requires_build.remove(cache_key);
            self.requires_prepare.remove(cache_key);
        }
        failed.len()
    }
}

/// Resolve the whole install's warm-cache lookups up front, returning a
/// `cache_key → Arc<cas_paths>` map the per-snapshot futures hit
/// synchronously. Keys with no row, an undecodable row, or a failed
/// integrity check are absent, and fall through to their per-snapshot
/// lookup. `integrity_check` says when that check runs.
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
    integrity_check: PrefetchIntegrityCheck,
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
        let decoded: Vec<DecodedPrefetchRow> = raw
            .into_par_iter()
            .filter_map(|(cache_key, bytes)| {
                decode_prefetch_row(
                    cache_key,
                    &bytes,
                    store_dir,
                    integrity_check,
                    &verified_files_cache,
                )
            })
            .collect();
        tracing::debug!(
            target: "pacquet::download",
            rows = decoded.len(),
            read_ms,
            decode_verify_ms = decode_start.elapsed().as_millis() as u64,
            "prefetch timings",
        );
        collect_prefetch_result(decoded)
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

/// Phase 2: decode one row's msgpackr-records bytes into a
/// [`PackageFilesIndex`], then run the integrity check. Both steps are
/// per-row CPU work with no shared state, so the caller fans them out
/// across rayon. With manifests included in the payload, decoding 1k+ rows
/// serially had become the dominant chunk of the prefetch wall
/// (single-threaded `spawn_blocking`); the par-iter recovers the per-row
/// parallelism the warm-batch link phase already uses.
///
/// The bundled manifest is split off the decoded entry via `Option::take` so
/// it travels back to the caller without an intermediate `Value::clone` of
/// the JSON tree — the verify function only inspects `files`, never
/// `manifest`.
fn decode_prefetch_row(
    cache_key: String,
    bytes: &[u8],
    store_dir: &'static StoreDir,
    integrity_check: PrefetchIntegrityCheck,
    verified_files_cache: &SharedVerifiedFilesCache,
) -> Option<DecodedPrefetchRow> {
    let mut entry: PackageFilesIndex = match pnpm_store_dir::decode_package_files_index(bytes) {
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
        // Left to the per-snapshot lookup, which is the one place that
        // reports the disagreement — as an error under
        // `strictStorePkgContentCheck`, as a warning without it. Skipping
        // here costs a re-read of the row in the latter case and nothing in
        // the former.
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
    let (verify_result, pending_check) = match integrity_check {
        PrefetchIntegrityCheck::Eager => (
            pnpm_store_dir::check_pkg_files_integrity(store_dir, entry, verified_files_cache),
            None,
        ),
        PrefetchIntegrityCheck::Deferred => {
            let (verify_result, pending_check) =
                pnpm_store_dir::defer_pkg_files_integrity(store_dir, entry);
            (verify_result, Some(pending_check))
        }
        PrefetchIntegrityCheck::Skip => {
            (pnpm_store_dir::build_file_maps_from_index(store_dir, entry), None)
        }
    };
    Some(DecodedPrefetchRow {
        cache_key,
        manifest,
        stored_requires_build,
        stored_requires_prepare,
        verify_result,
        pending_check,
    })
}

/// Fold the verified rows into the per-key maps the install path reads.
fn collect_prefetch_result(decoded: Vec<DecodedPrefetchRow>) -> PrefetchResult {
    let mut result = PrefetchResult {
        cas_paths: HashMap::with_capacity(decoded.len()),
        requires_build: HashMap::with_capacity(decoded.len()),
        ..PrefetchResult::default()
    };
    for row in decoded {
        let DecodedPrefetchRow {
            cache_key,
            manifest,
            stored_requires_build,
            stored_requires_prepare,
            mut verify_result,
            pending_check,
        } = row;
        if !verify_result.passed {
            continue;
        }
        if let Some(pending_check) = pending_check {
            result.pending_checks.insert(cache_key.clone(), pending_check);
        }
        let calculated_requires_build = stored_requires_build.unwrap_or_else(|| {
            manifest.as_deref().is_some_and(manifest_requires_build)
                || files_include_install_scripts(verify_result.files_map.keys())
        });
        if let Some(manifest) = manifest {
            result.manifests.insert(cache_key.clone(), manifest);
        }
        insert_side_effects(&mut result, &cache_key, &mut verify_result);
        result.requires_build.insert(cache_key.clone(), calculated_requires_build);
        if let Some(requires_prepare_value) = stored_requires_prepare {
            result.requires_prepare.insert(cache_key.clone(), requires_prepare_value);
        }
        result.cas_paths.insert(cache_key, Arc::new(verify_result.files_map));
    }
    result
}

/// An empty side-effects map is the same as none, so it is not recorded.
fn insert_side_effects(
    result: &mut PrefetchResult,
    cache_key: &str,
    verify_result: &mut pnpm_store_dir::VerifyResult,
) {
    if let Some(maps) = verify_result.side_effects_maps.take().filter(|maps| !maps.is_empty()) {
        result.side_effects_maps.insert(cache_key.to_string(), Arc::new(maps));
    }
    if let Some(diffs) = verify_result.side_effects.take().filter(|diffs| !diffs.is_empty()) {
        result.side_effects.insert(cache_key.to_string(), Arc::new(diffs));
    }
    if let Some(quarantine) = verify_result
        .remote_side_effects_quarantine
        .take()
        .filter(|quarantine| !quarantine.is_empty())
    {
        result.remote_side_effects_quarantine.insert(cache_key.to_string(), Arc::new(quarantine));
    }
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
/// `package_content_check` carries pnpm's `strictStorePkgContentCheck`
/// policy for package projections. [`PackageContentCheck::Strict`]
/// rejects an identity mismatch, [`PackageContentCheck::Warn`] reports
/// and uses the row, and [`PackageContentCheck::Skip`] omits this
/// npm-specific check for a raw archive projection.
///
/// `index` is opened once per install and passed in repeatedly, so the
/// `Connection::open` + PRAGMA cost is not paid per package.
pub(crate) async fn load_cached_cas_paths<Reporter: crate::Reporter>(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    cache_key: String,
    verify_store_integrity: bool,
    package_content_check: PackageContentCheck,
    verified_files_cache: SharedVerifiedFilesCache,
) -> Result<Option<HashMap<String, PathBuf>>, TarballError> {
    let Some(index) = index else { return Ok(None) };
    // Hold on to a copy of the cache key for the outer `JoinError` log,
    // since the task body moves the original in.
    let outer_cache_key = cache_key.clone();
    let result = tokio::task::spawn_blocking(move || {
        cached_row(
            &index,
            store_dir,
            &cache_key,
            verify_store_integrity,
            package_content_check,
            &verified_files_cache,
        )
    })
    .await;

    let row = match result {
        Ok(row) => row,
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
            return Ok(None);
        }
    };
    match row {
        CachedRow::Miss => Ok(None),
        CachedRow::Rejected(mismatch) => {
            Err(TarballError::UnexpectedPkgContentInStore { hint: mismatch.hint() })
        }
        CachedRow::Hit { cas_paths, mismatch } => {
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
    }
}

/// Look up one row and decide whether it can stand in for a download.
fn cached_row(
    index: &SharedReadonlyStoreIndex,
    store_dir: &'static StoreDir,
    cache_key: &str,
    verify_store_integrity: bool,
    package_content_check: PackageContentCheck,
    verified_files_cache: &SharedVerifiedFilesCache,
) -> CachedRow {
    let Some(entry) = read_row(index, cache_key) else {
        return CachedRow::Miss;
    };

    let mismatch = match package_content_check {
        PackageContentCheck::Strict | PackageContentCheck::Warn => {
            pnpm_store_dir::pkg_content_mismatch(entry.manifest.as_ref(), cache_key)
        }
        PackageContentCheck::Skip => None,
    };
    if package_content_check == PackageContentCheck::Strict
        && let Some(mismatch) = mismatch
    {
        return CachedRow::Rejected(mismatch);
    }

    let verify_result = if verify_store_integrity {
        pnpm_store_dir::check_pkg_files_integrity(store_dir, entry, verified_files_cache)
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
}

/// The row for `cache_key`, or `None` for a miss.
///
/// A poisoned mutex is treated as a cache miss rather than propagating the
/// panic: the `SELECT` is stateless, so the prior panic couldn't have left
/// the index in an inconsistent shape, and cache lookups are a best-effort
/// hint anyway — failing over to a fresh download is the more resilient
/// default than turning every subsequent snapshot into a crash.
fn read_row(index: &SharedReadonlyStoreIndex, cache_key: &str) -> Option<PackageFilesIndex> {
    let Ok(guard) = index.lock() else {
        tracing::debug!(
            target: "pacquet::download",
            ?cache_key,
            "store-index mutex poisoned; treating cache lookup as a miss",
        );
        return None;
    };
    guard.get(cache_key).ok().flatten()
}

/// Reuse a pre-projection-key runtime row only when its synthesized
/// `package.json` is byte-for-byte the one requested by this projection.
/// This keeps offline upgrades working without letting two runtime manifests
/// share the legacy row.
pub(crate) async fn load_legacy_synthesized_cas_paths<Reporter: crate::Reporter>(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    integrity: &str,
    package_id: &str,
    verify_store_integrity: bool,
    verified_files_cache: SharedVerifiedFilesCache,
    store_projection: ArchiveStoreProjection<'_>,
) -> Result<Option<HashMap<String, PathBuf>>, TarballError> {
    let Some((legacy_key, expected_manifest)) =
        store_projection.legacy_synthesized_store_row(integrity, package_id)
    else {
        return Ok(None);
    };
    let Some(cas_paths) = load_cached_cas_paths::<Reporter>(
        index,
        store_dir,
        legacy_key,
        verify_store_integrity,
        PackageContentCheck::Skip,
        verified_files_cache,
    )
    .await?
    else {
        return Ok(None);
    };
    let Some(package_json_path) = cas_paths.get("package.json") else {
        return Ok(None);
    };
    let Ok(cached_manifest) = tokio::fs::read(package_json_path).await else {
        return Ok(None);
    };
    if cached_manifest != expected_manifest {
        tracing::debug!(
            target: "pacquet::download",
            ?package_id,
            "legacy store row has a different synthesized manifest; treating it as a miss",
        );
        return Ok(None);
    }
    Ok(Some(cas_paths))
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
