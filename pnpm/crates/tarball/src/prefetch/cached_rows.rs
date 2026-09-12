use super::{
    ArchiveStoreProjection, GlobalLog, HashMap, LogEvent, LogLevel, PackageContentCheck,
    PackageFilesIndex, PathBuf, PkgContentMismatch, SharedReadonlyStoreIndex,
    SharedVerifiedFilesCache, StoreDir, TarballError,
};

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
    cached_row_paths::<Reporter>(row)
}

fn cached_row_paths<Reporter: crate::Reporter>(
    row: CachedRow,
) -> Result<Option<HashMap<String, PathBuf>>, TarballError> {
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
pub(super) fn read_raw_rows_under_lock(
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
