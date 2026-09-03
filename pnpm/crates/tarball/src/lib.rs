mod download;
mod error;
mod extract;
mod local_tarball;
mod prefetch;
mod zip_archive;

pub use download::*;
pub use error::*;
pub(crate) use extract::{
    GZIP_MAGIC, STREAM_ENTRY_BUFFER_MAX, STREAM_EXTRACT_COMPRESSED_THRESHOLD,
    STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD, allocate_tarball_buffer, apply_append_manifest,
    apply_placeholder_manifest, body_chunk_channel, decompress_gzip, extract_gzipped_tarball,
    is_eager_decode_limit_exceeded, non_gzip_body_error, normalize_bundled_manifest,
    oversized_manifest_error, stream_extract_gzipped_channel, tar_entry_payload,
};
pub use local_tarball::*;
pub use prefetch::*;
pub use zip_archive::*;

use std::{
    borrow::Cow,
    collections::HashMap,
    io::{self, Cursor, Read},
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

use dashmap::{DashMap, DashSet};
use pipe_trait::Pipe;
pub use pnpm_network::RetryOpts;
use pnpm_network::{AuthHeaders, ThrottledClient, UNPRIORITIZED};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{StoreDir, StoreIndexWriter, store_index_key};
use rayon::prelude::*;
use ssri::Integrity;
use tokio::sync::{Notify, RwLock, Semaphore};

/// Ceiling on a single eager buffer reservation sized from untrusted
/// archive metadata — `dist.unpackedSize` in registry metadata and an
/// entry's `uncompressed_size` in a zip central directory. Both are
/// attacker-controlled, and post-download work runs concurrently (see
/// [`post_download_semaphore`]), so whatever one task reserves up front
/// is multiplied across every task in flight.
///
/// Bounds the eager reservation only, never the output: both consumers
/// grow their buffer on demand, so an archive larger than the ceiling
/// still decodes in full.
const MAX_UNTRUSTED_PREALLOC_BYTES: usize = 64 * 1024 * 1024;

fn auth_header_for_package_download(
    auth_headers: &AuthHeaders,
    package_url: &str,
    package_id: &str,
) -> Option<String> {
    if package_id.starts_with("node@runtime:") {
        auth_headers.for_secure_url_with_package(package_url, Some(package_id))
    } else {
        auth_headers.for_url_with_package(package_url, Some(package_id))
    }
}

/// Cap on concurrent post-download tarball work (SHA-512 of the whole
/// tarball + gzip inflate + per-file SHA-512 + CAFS writes). The body is
/// CPU-bound with some blocking FS I/O, and putting it on
/// `tokio::task::spawn_blocking` makes the default 512-thread blocking
/// pool available — but async fan-out across `try_join_all` routinely
/// fires hundreds of these at once on a 1352-snapshot install, which
/// thrashes small CI runners. Past "Download completed" a 2-CPU GitHub
/// Actions runner wedged between decompress-close and `Checksum verified`
/// on [#269] until the step timeout. `num_cpus * 2` (floor 4) keeps enough
/// work in flight to overlap per-file FS writes with SHA on another task
/// without oversubscribing the cores.
///
/// [#269]: https://github.com/pnpm/pacquet/pull/269
fn post_download_semaphore() -> &'static Semaphore {
    static SEM: LazyLock<Semaphore> =
        LazyLock::new(|| Semaphore::new(num_cpus::get().saturating_mul(2).max(4)));
    &SEM
}

/// Admission cap for the extract-while-downloading path (see
/// [`download`]): how many downloads may hold a blocking thread that
/// extracts their body as it arrives. Deliberately separate from
/// [`post_download_semaphore`] because a streaming extractor holds its slot
/// for the whole body transfer (mostly parked between chunks), so
/// sharing the post-download permits would starve the eager
/// extractions that hold one only for a burst of CPU. The cap uses
/// [`std::thread::available_parallelism`] with a minimum of two permits
/// so cgroup and CPU-quota limits are respected. Admission uses
/// `try_acquire`; a download with no free slot buffers its body instead.
fn streaming_extract_semaphore() -> &'static Semaphore {
    static SEM: LazyLock<Semaphore> = LazyLock::new(|| {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        Semaphore::new(cores.max(2))
    });
    &SEM
}

/// Dedicated rayon pool for the per-file CAS-write phase of extraction
/// ([`crate::extract::extract_tarball_entries`]).
///
/// Separate from the global pool because the install overlaps
/// extraction with linking, and the linker runs on the global pool:
/// sharing it would let hundreds of tarballs finishing at once queue
/// ahead of the linker and stall it for seconds.
///
/// Sized to the core count, the work being CPU-bound (SHA-512 plus the
/// CAFS write). `None` if the pool cannot be built, and the caller
/// falls back to the global pool.
fn cas_write_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: LazyLock<Option<rayon::ThreadPool>> = LazyLock::new(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get().max(1))
            .thread_name(|index| format!("cas-write-{index}"))
            .build()
            .map_err(|error| {
                tracing::warn!(
                    target: "pacquet::download",
                    ?error,
                    "failed to build the dedicated CAS-write pool; falling back to the global rayon pool",
                );
            })
            .ok()
    });
    POOL.as_ref()
}

/// Per-package callback that decides whether a given archive entry
/// (path relative to the archive's top-level directory, after the
/// `prefix` strip on zip archives, after the `package/` strip on
/// npm tarballs) should be excluded from the CAS write.
///
/// Implements the `ignoreFilePattern` / `archiveFilters` behavior.
/// Pacquet uses a callback rather than a regex so the caller can
/// hand-code the filter without pulling a regex engine into
/// `pnpm-tarball`; the canonical Node-runtime filter lives at
/// the install-dispatch site (Slice D) where it's constructed once
/// per fetch.
///
/// The callback receives the *cleaned* path (post-prefix strip,
/// `to_string_lossy()` already applied), so its inputs are stable
/// strings keyed the same way pnpm keys the equivalent filter.
pub type IgnoreEntryFilter = dyn Fn(&str) -> bool + Send + Sync;

/// Value of the cache.
#[derive(Debug, Clone)]
pub enum CacheValue {
    /// The package is being processed.
    InProgress(Arc<Notify>),
    /// The package is saved.
    Available(Arc<HashMap<String, PathBuf>>),
    /// The owning fetch failed; concurrent waiters wake up to this
    /// instead of `Available` and surface a sibling-fetch-failed
    /// error rather than blocking on the `Notify` forever. The
    /// originating [`TarballError`] cannot be cloned past the owner
    /// (it's wrapped in `reqwest::Error` / IO chains that aren't
    /// `Clone`), so waiters return their own variant — see
    /// [`TarballError::SiblingFetchFailed`].
    Failed,
}

/// Internal in-memory cache of tarballs.
///
/// The key is the tarball URL, prefixed for revision-addressed fetches so
/// redirect and retry policies never share a result.
pub type MemCache = DashMap<String, Arc<RwLock<CacheValue>>>;

/// Install-scoped set of store-index cache keys
/// (`store_index_key(integrity, pkg_id)`) whose package status
/// (`fetched` or `found_in_store`) has already been emitted during this
/// install.
///
/// The resolve-time prefetcher emits download/cache-hit progress as soon
/// as it knows the outcome, then records the key here. The later
/// virtual-store warm batch still emits `resolved`, but skips the second
/// package status for recorded keys, so progress is timely without
/// double-counting. See <https://github.com/pnpm/pnpm/issues/12235>.
pub type ReportedProgressKeys = DashSet<String>;

/// Shared handle to a [`ReportedProgressKeys`] set, allocated once per
/// install and shared between early fetchers and the later install-pass
/// reporter.
pub type SharedReportedProgressKeys = Arc<ReportedProgressKeys>;

/// A verified archive's CAFS files and bundled package metadata.
#[derive(Debug, Clone)]
pub struct FetchedTarball {
    pub integrity: Integrity,
    pub files_map: HashMap<String, PathBuf>,
    pub manifest: Option<serde_json::Value>,
    pub requires_build: bool,
}

impl<'a> DownloadTarballToStore<'a> {
    /// Execute the subroutine with an in-memory cache.
    ///
    /// # Caller invariant: stable filter per URL
    ///
    /// The cache is keyed on `package_url` and whether the request uses the
    /// revision-addressed network policy. Within either policy, a second
    /// caller fetching the same URL with a different [`ignore_file_pattern`]
    /// silently receives the map the first caller's filter produced. Every
    /// fetch of a URL must use the same filter. Nothing enforces this; today
    /// it holds because URLs encode `(name, version, integrity)` and filters
    /// are keyed by package name.
    ///
    /// [`ignore_file_pattern`]: DownloadTarballToStore::ignore_file_pattern
    pub async fn run_with_mem_cache<Reporter: self::Reporter>(
        self,
        mem_cache: &'a MemCache,
    ) -> Result<Arc<HashMap<String, PathBuf>>, TarballError> {
        self.run_with_mem_cache_inner::<Reporter>(mem_cache, false).await
    }

    /// Execute a registry revision fetch with the shared in-memory cache.
    /// The network path performs exactly one GET and rejects redirects.
    pub async fn run_revision_addressed_with_mem_cache<Reporter: self::Reporter>(
        self,
        mem_cache: &'a MemCache,
    ) -> Result<Arc<HashMap<String, PathBuf>>, TarballError> {
        self.run_with_mem_cache_inner::<Reporter>(mem_cache, true).await
    }

    async fn run_with_mem_cache_inner<Reporter: self::Reporter>(
        self,
        mem_cache: &'a MemCache,
        revision_addressed: bool,
    ) -> Result<Arc<HashMap<String, PathBuf>>, TarballError> {
        let &DownloadTarballToStore {
            package_url,
            package_id,
            package_integrity,
            prefetched_cas_paths,
            requester,
            ..
        } = &self;
        let mem_cache_key = if revision_addressed {
            format!("revision-addressed:{package_url}")
        } else {
            package_url.to_string()
        };
        let cache_key = store_index_cache_key(package_integrity, package_id);
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());

        // Hands the `Arc` on without deep-cloning the per-file map:
        // on a warm install every snapshot takes this path, and by 1k+
        // snapshots that clone dominates the memory traffic. The `Arc`
        // is also stashed in `mem_cache` by URL so peer-resolved
        // variants of one package share it.
        if let Some(prefetched) = prefetched_cas_paths
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(cas_paths) = prefetched.get(cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "Reusing prefetched CAFS entry — skipping download (warm-cache fast path)",
            );
            emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
            let cas_paths = Arc::clone(cas_paths);
            let cache_lock = Arc::new(RwLock::new(CacheValue::Available(Arc::clone(&cas_paths))));
            mem_cache.insert(mem_cache_key, cache_lock);
            return Ok(cas_paths);
        }

        // QUESTION: I see no copying from existing store_dir, is there such mechanism?
        // TODO: If it's not implemented yet, implement it

        // Claim ownership atomically so concurrent callers cannot both start
        // the one network fetch for this URL. The entry guard is dropped when
        // this match returns, before either branch awaits the cache lock.
        let (cache_lock, owner_notify) = match mem_cache.entry(mem_cache_key.clone()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => (Arc::clone(entry.get()), None),
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let notify = Arc::new(Notify::new());
                let cache_lock = notify
                    .pipe_ref(Arc::clone)
                    .pipe(CacheValue::InProgress)
                    .pipe(RwLock::new)
                    .pipe(Arc::new);
                entry.insert(Arc::clone(&cache_lock));
                (cache_lock, Some(notify))
            }
        };
        match owner_notify {
            None => {
                // `pnpm:progress` fires exactly once per URL — only the
                // first writer's `run_without_mem_cache` call emits.
                // Later waiters on the same cache slot do not re-trigger
                // the emit.
                //
                // Read-lock the state read: the variant inspection below
                // doesn't mutate anything, and a `write().await` would
                // serialize every late visitor for a popular tarball
                // (e.g. dozens of peer-suffix variants of the same
                // package) behind a single exclusive guard, even though
                // they're all just observing the in-progress / available
                // flag. The owner branch below is the only writer; the
                // RwLock's reader-writer fairness guarantees the owner
                // still makes progress.
                let notify = match &*cache_lock.read().await {
                    CacheValue::Available(cas_paths) => {
                        // The first owner already reported its package
                        // status. If the caller supplied a shared
                        // progress set, this emit is skipped for keys the
                        // owner reported; otherwise preserve the legacy
                        // per-caller cache-hit progress.
                        emit_progress_found_in_store::<Reporter>(
                            package_id,
                            requester,
                            progress_key,
                        );
                        return Ok(Arc::clone(cas_paths));
                    }
                    CacheValue::InProgress(notify) => Arc::clone(notify),
                    CacheValue::Failed => {
                        // The owner already finished and failed; surface
                        // immediately rather than parking on the Notify.
                        return Err(TarballError::SiblingFetchFailed {
                            url: package_url.to_string(),
                        });
                    }
                };

                tracing::info!(target: "pacquet::download", ?package_url, "Wait for cache");
                loop {
                    // Register with the `Notify` *before* re-checking the
                    // slot. `notify_waiters` stores no permit — it wakes
                    // only `Notified` futures already registered at that
                    // instant — and the read guard from the `InProgress`
                    // observation above is released before this point, so
                    // the owner's flip-and-notify can land in between.
                    // Checking first and registering after loses that
                    // wakeup and parks this task forever (nothing ever
                    // notifies the slot again once it is terminal).
                    let notified = notify.notified();
                    let mut notified = std::pin::pin!(notified);
                    notified.as_mut().enable();
                    match &*cache_lock.read().await {
                        CacheValue::Available(cas_paths) => {
                            // Same rationale as the pre-wait `Available`
                            // branch above.
                            emit_progress_found_in_store::<Reporter>(
                                package_id,
                                requester,
                                progress_key,
                            );
                            return Ok(Arc::clone(cas_paths));
                        }
                        CacheValue::Failed => {
                            return Err(TarballError::SiblingFetchFailed {
                                url: package_url.to_string(),
                            });
                        }
                        // The owner notifies only after flipping the slot
                        // to `Available` or `Failed`, so a wake with the
                        // slot still `InProgress` cannot happen — but a
                        // stale registration completing early is harmless
                        // either way: re-register and park again.
                        CacheValue::InProgress(_) => {}
                    }
                    notified.await;
                }
            }
            Some(notify) => {
                // Run the actual fetch and cleanup in either branch. On
                // error the cache slot must transition to `Failed` and
                // we must `notify_waiters` so concurrent requesters
                // wake up and surface a sibling-fetch error instead of
                // parking on the Notify forever.
                // Ordinary fetches remove the failed slot so a later caller
                // can retry. A revision-addressed fetch keeps it terminal for
                // this install, preserving the protocol's one-GET contract.
                let result = self.run_without_mem_cache_inner::<Reporter>(revision_addressed).await;
                match result {
                    Ok(cas_paths) => {
                        let cas_paths = Arc::new(cas_paths);
                        let mut cache_write = cache_lock.write().await;
                        *cache_write = CacheValue::Available(Arc::clone(&cas_paths));
                        drop(cache_write);
                        notify.notify_waiters();
                        Ok(cas_paths)
                    }
                    Err(err) => {
                        let mut cache_write = cache_lock.write().await;
                        *cache_write = CacheValue::Failed;
                        drop(cache_write);
                        if !revision_addressed {
                            mem_cache.remove(&mem_cache_key);
                        }
                        notify.notify_waiters();
                        Err(err)
                    }
                }
            }
        }
    }

    /// Execute the subroutine without an in-memory cache.
    pub async fn run_without_mem_cache<Reporter: self::Reporter>(
        &self,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        self.run_without_mem_cache_inner::<Reporter>(false).await
    }

    /// Execute a registry revision fetch without the in-memory cache.
    /// The network path performs exactly one GET and rejects redirects.
    pub async fn run_revision_addressed_without_mem_cache<Reporter: self::Reporter>(
        &self,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        self.run_without_mem_cache_inner::<Reporter>(true).await
    }

    async fn run_without_mem_cache_inner<Reporter: self::Reporter>(
        &self,
        revision_addressed: bool,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        let &DownloadTarballToStore {
            store_dir,
            package_integrity,
            package_url,
            package_id,
            requester,
            verify_store_integrity,
            strict_store_pkg_content_check,
            prefetched_cas_paths,
            ..
        } = self;

        // Before hitting the network, check the SQLite store index: if the
        // tarball is already in the CAFS we can reuse its per-file paths
        // and skip the download entirely. This is the payoff of the v11
        // store migration (<https://github.com/pnpm/pacquet/issues/244>) — pnpm and pacquet share `index.db`, so a
        // previous install of the same (integrity, pkg_id) pair leaves an
        // entry we can read back here.
        //
        // The lookup is best-effort. A missing `index.db`, a missing row,
        // an undecodable entry, or any CAFS file that has gone missing
        // from disk all fall through to the download path below.
        let cache_key = store_index_cache_key(package_integrity, package_id);
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());
        // Deep-clones the inner map, unlike the `Arc`-preserving path
        // in `run_with_mem_cache`: this signature returns an owned
        // `HashMap`, and widening it would reach into
        // `DownloadTarballToStore`'s return type. Affordable because
        // only cache-miss snapshots reach here, where the clone is
        // dwarfed by the download it avoids.
        if let Some(prefetched) = prefetched_cas_paths
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(cas_paths) = prefetched.get(cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "Reusing prefetched CAFS entry — skipping download",
            );
            emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
            return Ok((**cas_paths).clone());
        }
        if let Some(cache_key) = cache_key.clone() {
            let cached = load_cached_cas_paths::<Reporter>(
                self.store_index.clone(),
                store_dir,
                cache_key,
                verify_store_integrity,
                strict_store_pkg_content_check,
                Arc::clone(&self.verified_files_cache),
            )
            .await?;
            if let Some(cas_paths) = cached {
                tracing::info!(target: "pacquet::download", ?package_url, ?package_id, "Reusing cached CAFS entry — skipping download");
                emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
                return Ok(cas_paths);
            }
        }
        self.fetch_and_extract_inner::<Reporter>(false, revision_addressed)
            .await
            .map(|result| result.files_map)
    }

    /// Fetch the requested archive, verify any expected integrity, and return its CAFS files.
    /// Unlike [`Self::run_without_mem_cache`], this does not reuse cached content.
    /// Archives without an expected integrity are indexed by their computed SHA-512.
    pub async fn fetch_and_extract<Reporter: self::Reporter>(
        &self,
    ) -> Result<FetchedTarball, TarballError> {
        self.fetch_and_extract_inner::<Reporter>(true, false).await
    }

    async fn fetch_and_extract_inner<Reporter: self::Reporter>(
        &self,
        record_computed_integrity: bool,
        revision_addressed: bool,
    ) -> Result<FetchedTarball, TarballError> {
        let &DownloadTarballToStore {
            http_client,
            store_dir,
            package_integrity,
            package_unpacked_size,
            package_file_count,
            package_url,
            package_id,
            requester,
            retry_opts,
            auth_headers,
            append_manifest,
            ..
        } = self;
        let cache_key = store_index_cache_key(package_integrity, package_id);
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());
        let store_index_writer = self.store_index_writer.clone();
        // `Option<Arc<IgnoreEntryFilter>>` isn't `Copy`, so it can't
        // ride along in the deref-destructure above. `.clone()`
        // here bumps the Arc refcount — cheap, and the trait
        // object is shared with the install dispatcher that
        // owns the original.
        let ignore_file_pattern = self.ignore_file_pattern.clone();

        // Offline-mode gate: nothing past this point is served from a
        // cache. pnpm gates only its metadata path on `--offline`;
        // pacquet has no metadata path on the frozen-install flow, so
        // the gate lands here. Error rather than fall through to the
        // network — same shape as pnpm's `ERR_PNPM_NO_OFFLINE_META`,
        // scoped to tarballs because that's what pacquet's frozen
        // install needs network for.
        if self.offline && local_file_tarball_path(package_url).is_none() {
            tracing::warn!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "offline mode: tarball missing from local store; refusing network fetch",
            );
            return Err(TarballError::NoOfflineTarball {
                package_id: package_id.to_string(),
                url: package_url.to_string(),
            });
        }

        tracing::info!(target: "pacquet::download", ?package_url, "New cache");

        // Run the full fetch + integrity + extract pipeline under
        // pnpm's retry policy: a single retried closure wraps both the
        // network side and the integrity-check + extract side, so a
        // flaky transfer that survives TCP framing but fails the
        // SHA-512 hash or trips gzip / tar parsing recovers via
        // re-fetch instead of aborting the install
        // (<https://github.com/pnpm/pacquet/issues/259>). Only HTTP 401 / 403 / 404 fail fast — see
        // [`is_transient_error`].
        let (computed_integrity, mut cas_paths, mut pkg_files_idx) =
            fetch_and_extract_with_retry::<Reporter>(
                http_client,
                package_url,
                package_integrity,
                package_unpacked_size,
                download_priority(package_unpacked_size, package_file_count),
                package_id,
                requester,
                store_dir,
                retry_opts,
                auth_headers,
                ignore_file_pattern,
                progress_key,
                revision_addressed,
            )
            .await?;

        // Fold the synthesized runtime `package.json` into the row before
        // it is persisted, so warm reinstalls (which read the row) get it.
        if let Some(manifest_bytes) = append_manifest {
            apply_append_manifest(store_dir, manifest_bytes, &mut cas_paths, &mut pkg_files_idx)?;
        }
        apply_placeholder_manifest(store_dir, &mut cas_paths, &mut pkg_files_idx)?;

        let manifest = pkg_files_idx.manifest.clone();
        // Only legacy cache rows omit this; fresh extraction always records it.
        let requires_build =
            pkg_files_idx.requires_build.expect("fresh extraction records build requirement");

        // Hand the per-tarball files index off to the shared writer task
        // from <https://github.com/pnpm/pacquet/pull/265> *after* the retry loop returns, so transient failures
        // don't queue a half-built row that a successful retry would
        // duplicate. `queue` is a non-blocking `UnboundedSender::send`;
        // the writer task owns one connection and batches whatever it
        // drains in one `BEGIN IMMEDIATE; ... ; COMMIT`. `None` means the
        // writer failed to open or the caller handed us none — the row
        // is dropped with a `warn!` and the next install misses on this
        // cache key, matching the read path's stance.
        let cache_key = cache_key.or_else(|| {
            record_computed_integrity
                .then(|| store_index_key(&computed_integrity.to_string(), package_id))
        });
        match (cache_key, store_index_writer) {
            (Some(index_key), Some(writer)) => writer.queue(index_key, pkg_files_idx),
            (Some(index_key), None) => tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this tarball",
            ),
            (None, _) => tracing::debug!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "resolution carries no integrity; skipping index row for this tarball",
            ),
        }

        Ok(FetchedTarball {
            integrity: computed_integrity,
            files_map: cas_paths,
            manifest,
            requires_build,
        })
    }
}

/// Outcome of [`FetchTarballForResolution::run`]: the sha512 integrity
/// computed from the downloaded tarball and the bundled manifest read
/// from its `package.json`. The extracted CAFS paths are not returned —
/// they are stashed in the shared [`MemCache`] keyed by URL so the
/// install pass reuses them without re-downloading.
#[derive(Debug)]
pub struct ResolvedTarball {
    pub integrity: Integrity,
    pub manifest: Option<serde_json::Value>,
}

/// Download a remote tarball during *resolution*, compute its sha512
/// integrity, extract it to the store, and read its bundled manifest.
///
/// Remote (non-registry) https-tarball direct dependencies carry no
/// name/version/integrity at resolve time — those live in the tarball's
/// `package.json`, learned only after the fetch. pacquet builds the
/// lockfile before the install pass, so the `TarballResolver` must
/// fetch here to fill `manifest` + `integrity` into its
/// `ResolveResult`. Passing a `mem_cache` warms it (keyed by URL) so
/// the install pass's
/// [`DownloadTarballToStore::run_with_mem_cache`] reuses the extraction
/// without a second download.
pub struct FetchTarballForResolution<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub package_url: &'a str,
    /// Package identity used for scoped auth lookup and for the
    /// store-index row this fetch writes. Must be the `pkg_id` the
    /// install pass derives from the lockfile entry — the bare URL for a
    /// remote tarball — or the two passes file the same content under
    /// two rows.
    pub package_id: &'a str,
    pub auth_headers: &'a AuthHeaders,
    pub retry_opts: RetryOpts,
    /// Directory *within* the archive holding the package, for a
    /// git-hosted dep that points at one directory of a repo
    /// (`#path:/packages/foo`). The archive spans the whole repo, so
    /// the root `package.json` describes the repo, not the package —
    /// read the manifest from here instead. `None` reads the root.
    ///
    /// Matches the resolution's `path` field verbatim, leading slash
    /// and all.
    ///
    /// Setting this suppresses the store-index row: the extracted
    /// index describes the archive, not the named subpackage, so
    /// there is no row to write that the key would honestly describe.
    pub manifest_subdir: Option<&'a str>,
}

impl FetchTarballForResolution<'_> {
    pub async fn run<Reporter: self::Reporter>(
        self,
        mem_cache: Option<&MemCache>,
    ) -> Result<ResolvedTarball, TarballError> {
        let FetchTarballForResolution {
            http_client,
            store_dir,
            store_index_writer,
            package_url,
            package_id,
            auth_headers,
            retry_opts,
            manifest_subdir,
        } = self;

        // Resolve-time tarball fetches compute integrity from bytes and
        // gate the dependency walk, so they use the same priority class as
        // packument requests instead of queuing behind sized downloads.
        let (integrity, mut cas_paths, mut pkg_files_idx) =
            fetch_and_extract_with_retry::<Reporter>(
                http_client,
                package_url,
                None,
                None,
                UNPRIORITIZED,
                package_id,
                package_url,
                store_dir,
                retry_opts,
                auth_headers,
                None,
                None,
                false,
            )
            .await?;
        apply_placeholder_manifest(store_dir, &mut cas_paths, &mut pkg_files_idx)?;

        let manifest = match manifest_subdir {
            Some(subdir) => read_subdir_manifest(&cas_paths, subdir).await?,
            None => pkg_files_idx.manifest.clone(),
        };

        // A subdirectory package gets no row. Its key would name the
        // subpackage while `pkg_files_idx` describes the whole archive
        // — the repo's manifest and every repo file — and a row whose
        // key and payload disagree is worse than none: consumers that
        // trust `PackageFilesIndex.manifest` / `files` to match the key
        // (bin linking, file materialization) would read the repo.
        // Nothing needs this row. A git-hosted archive — the only shape
        // carrying a subdirectory — is addressed by
        // `git_hosted_store_index_key` once the install pass has run
        // `prepare` over it, and both the graph prefetch and the
        // warm-store reuse map skip git-hosted entries.
        if manifest_subdir.is_none() {
            // Key the row by the caller's `package_id` — the same
            // `pkg_id` the install pass derives from the lockfile entry.
            // Deriving a `name@version` from the bundled manifest instead
            // would file a remote tarball under a key nothing ever reads,
            // leaving the install pass to write a second row for the same
            // content.
            let index_key = store_index_key(&integrity.to_string(), package_id);
            if let Some(writer) = store_index_writer {
                writer.queue(index_key, pkg_files_idx);
            } else {
                tracing::warn!(
                    target: "pacquet::download",
                    ?index_key,
                    "no shared store-index writer; skipping index row for this resolve-time tarball",
                );
            }
        }

        if let Some(mem_cache) = mem_cache {
            let cache_lock = Arc::new(RwLock::new(CacheValue::Available(Arc::new(cas_paths))));
            mem_cache.insert(package_url.to_string(), cache_lock);
        }

        Ok(ResolvedTarball { integrity, manifest })
    }
}

#[cfg(test)]
mod tests;
