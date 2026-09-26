use crate::PrefetchedCasPaths;
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndexWriter,
};
use ssri::Integrity;
use std::sync::Arc;

#[derive(Clone)]
pub struct ArchiveStoreContext<'a> {
    pub dir: &'static StoreDir,
    /// Shared read-only handle to the `SQLite` store index. `None` when the
    /// store does not (yet) have an `index.db`, in which case every cache
    /// lookup short-circuits to a network fetch. Callers open this once per
    /// install and pass the same handle to every [`crate::IngestTarballToStore`]
    /// so we don't reopen the DB per package.
    pub index: Option<SharedReadonlyStoreIndex>,
    /// Handle to the batched store-index writer. Each successful tarball
    /// extraction queues one `(key, PackageFilesIndex)` row; a single
    /// writer task drains the channel and flushes batches of up to 256 in
    /// one transaction each, so the whole install goes through one
    /// `Connection::open` and a handful of WAL commits. Opening a
    /// connection per tarball instead would saturate tokio's blocking
    /// pool — 500+ threads on a 1352-snapshot install, see [#263].
    /// `None` degrades to "skip index row", matching the read
    /// side's stance: install still succeeds, the next install misses on
    /// this cache key and re-downloads.
    ///
    /// [#263]: https://github.com/pnpm/pacquet/issues/263
    pub index_writer: Option<Arc<StoreIndexWriter>>,
    /// Mirrors pnpm's `verify-store-integrity` / `verifyStoreIntegrity`
    /// setting. When `true` (pnpm's default) each cached CAFS file is
    /// stat'ed and optionally re-hashed before reuse. When `false` the
    /// index is trusted and the import fails lazily if a blob is
    /// missing — trades the per-file stat / optional rehash for the
    /// risk that a mutated or corrupt store serves stale content until
    /// the next integrity-full install. Whether that translates into a
    /// wall-time win depends on the workload; the per-snapshot stat
    /// isn't the bottleneck on the benchmarks this repo tracks (see
    /// [#273]), but cutting the syscall count is still correct.
    ///
    /// [#273]: https://github.com/pnpm/pacquet/issues/273
    pub verify_integrity: bool,
    /// Mirrors pnpm's `strictStorePkgContentCheck` setting (default
    /// `true`). A store row whose bundled manifest names a package other
    /// than the row's key does fails the install under it, and is used
    /// with a warning without it. See
    /// [`pnpm_store_dir::pkg_content_mismatch`] for what counts as a
    /// disagreement.
    pub strict_pkg_content_check: bool,
    /// Install-scoped dedup cache shared across every cached-tarball
    /// lookup. Ports pnpm's `verifiedFilesCache: Set<string>`: a CAFS
    /// path that one snapshot's verify pass has already stat'ed (and
    /// optionally re-hashed) gets skipped when the next snapshot
    /// touches the same blob. Without it pacquet was paying the
    /// per-file stat in `check_pkg_files_integrity` once per
    /// (snapshot × file) instead of once per (file). Allocate one
    /// `Arc<DashSet<PathBuf>>` at install bootstrap and pass the same
    /// handle to every [`crate::IngestTarballToStore`].
    pub verified_files_cache: SharedVerifiedFilesCache,
    /// Pre-fetched cache lookups built once at install start
    /// ([`crate::prefetch::prefetch_cas_paths`]). When `Some`, this is consulted first;
    /// the per-snapshot `SQLite` + integrity-check round-trip is skipped
    /// for every key already resolved by the prefetch.
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Read-only store to copy a package from when this store misses it
    /// (`fallbackStoreDir`). Its files are copied into [`Self::dir`], each
    /// checked against its recorded digest, before the package is used.
    pub fallback_dir: Option<&'static StoreDir>,
}

#[derive(Clone, Copy)]
pub struct ArchiveFetchOptions<'a> {
    pub http_client: &'a ThrottledClient,
    /// URL-keyed `Authorization` header lookup, built from the parsed
    /// `.npmrc` creds. Resolved per request so a tarball served from a
    /// different host than the registry still picks up its own header.
    pub auth_headers: &'a AuthHeaders,
    /// Per-attempt retry budget for the tarball pipeline, driven by
    /// pnpm's `fetch-retries*` knobs. Network resets, timeouts, integrity
    /// mismatches, and archive decode errors retry ([#259]). HTTP 401,
    /// 403, 404, untrusted certificates, and a full local store do not.
    ///
    /// [#259]: https://github.com/pnpm/pacquet/issues/259
    pub retry_opts: RetryOpts,
    /// `offline` from `Config`. When `true` and both the warm
    /// prefetch (`prefetched_cas_paths`) and the `SQLite` `index.db`
    /// lookup (`load_cached_cas_paths`) miss, the fetcher fails fast
    /// with [`crate::TarballError::NoOfflineTarball`] rather than hitting
    /// the registry. The `--offline` flag gates the metadata-fetch
    /// path in pnpm; pacquet has no metadata-fetch path on the
    /// frozen-install flow (the lockfile pins every resolution), so
    /// this gate is pacquet's most useful interpretation of the flag
    /// for frozen installs.
    pub offline: bool,
}

#[derive(Clone, Copy)]
pub struct TarballPackage<'a> {
    /// Expected hash of the tarball bytes. `None` for a lockfile entry
    /// recording no `integrity`, the shape pnpm wrote for git-host
    /// archives before it pinned their hash; pnpm fetches those
    /// unverified, so pacquet does too.
    ///
    /// [`crate::IngestTarballToStore::run_without_mem_cache`] neither reads nor writes an
    /// `index.db` row for an unpinned archive. Its fallback key belongs
    /// to the *prepared* file set that `GitHostedTarballFetcher` writes
    /// after running `prepare` + packlist; claiming it here would leave
    /// the raw archive in the row whenever that pass failed.
    /// [`crate::IngestTarballToStore::fetch_and_extract`] can instead index a plain archive by
    /// its computed integrity.
    pub integrity: Option<&'a Integrity>,
    pub unpacked_size: Option<usize>,
    /// `dist.fileCount` when the registry published one. Combined with
    /// `unpacked_size` into the download's queueing priority —
    /// per-file pipeline overhead (CAS write syscalls, hashing) makes a
    /// many-small-files package as slow to finish as a much larger
    /// few-files one.
    pub file_count: Option<usize>,
    pub url: &'a str,
    /// Stable identifier for the package, e.g. `"{name}@{version}"`. Paired
    /// with `integrity` to form the `SQLite` index key per pnpm v11's
    /// `storeIndexKey`, when there is an integrity to pair it with.
    pub id: &'a str,
}

#[derive(Clone, Copy)]
pub struct ZipArchivePackage<'a> {
    /// Reject an advertised or streamed ZIP body above this limit. `None` leaves it unrestricted.
    pub max_bytes: Option<usize>,
    pub integrity: &'a Integrity,
    pub url: &'a str,
    pub id: &'a str,
}
