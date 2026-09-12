//! Bulk store-index prefetch.
//!
//! Reads every already-present package's CAS row in one pass before
//! the install fans out, so the warm batch can skip per-package
//! store-index lookups entirely.

pub(crate) use cached_rows::{load_cached_cas_paths, load_legacy_synthesized_cas_paths};

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
        prefetch_cas_paths_blocking(
            &index,
            store_dir,
            &cache_keys,
            integrity_check,
            &verified_files_cache,
        )
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

fn prefetch_cas_paths_blocking(
    index: &SharedReadonlyStoreIndex,
    store_dir: &'static StoreDir,
    cache_keys: &[String],
    integrity_check: PrefetchIntegrityCheck,
    verified_files_cache: &SharedVerifiedFilesCache,
) -> PrefetchResult {
    let read_start = std::time::Instant::now();
    let Some(raw) = read_raw_rows_under_lock(index, cache_keys) else {
        return PrefetchResult::default();
    };
    let read_ms = read_start.elapsed().as_millis() as u64;

    let decode_start = std::time::Instant::now();
    let decoded: Vec<DecodedPrefetchRow> = raw
        .into_par_iter()
        .filter_map(|(cache_key, bytes)| {
            decode_prefetch_row(cache_key, &bytes, store_dir, integrity_check, verified_files_cache)
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
    let mut entry = decode_matching_prefetch_entry(&cache_key, bytes)?;
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

/// A mismatched row is left to the per-snapshot lookup, which reports
/// the disagreement according to strictStorePkgContentCheck.
fn decode_matching_prefetch_entry(cache_key: &str, bytes: &[u8]) -> Option<PackageFilesIndex> {
    let entry: PackageFilesIndex = match pnpm_store_dir::decode_package_files_index(bytes) {
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
    if let Some(mismatch) = pnpm_store_dir::pkg_content_mismatch(entry.manifest.as_ref(), cache_key)
    {
        tracing::debug!(
            target: "pacquet::download",
            ?cache_key,
            expected = mismatch.expected,
            actual = mismatch.actual,
            "store-index row holds another package; leaving it to the per-snapshot lookup",
        );
        return None;
    }
    Some(entry)
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

mod cached_rows;
use cached_rows::read_raw_rows_under_lock;
