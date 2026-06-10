//! Background tarball downloads for the pnpr client path.
//!
//! [`TarballPrefetcher`] fires a download as each `package` frame streams
//! in from `/v1/resolve` so the fetch overlaps the *server's* resolution
//! ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)). It is
//! the streaming-client analogue of [`crate::PrefetchingResolver`] (the
//! local fresh-install prefetcher), but independent: the resolver path
//! reports prefetch progress through the install reporter, whereas this
//! one runs silently and lets the frozen materialization install emit
//! progress as it consumes each tarball.
//!
//! Each download lands its result in the shared [`MemCache`] keyed by
//! tarball URL; the later install pass picks it up via
//! [`DownloadTarballToStore::run_with_mem_cache`] (an immediate
//! `CacheValue::Available` hit, or a brief park on the per-URL `Notify`
//! while the prefetch finishes).

use crate::retry_config::retry_opts_from_config;
use dashmap::DashSet;
use pacquet_config::Config;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexError,
    StoreIndexWriter,
};
use pacquet_tarball::{DownloadTarballToStore, MemCache, RetryOpts};
use ssri::Integrity;
use std::sync::Arc;

/// One background tarball download. Every field is owned (an `Arc`
/// clone or a `Copy` scalar) so the spawned task captures an
/// independent set without borrowing the caller's scope.
pub(crate) struct TarballDownload {
    pub http_client: Arc<ThrottledClient>,
    pub mem_cache: Arc<MemCache>,
    pub store_dir: &'static StoreDir,
    pub store_index: Option<SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub verified_files_cache: SharedVerifiedFilesCache,
    pub auth_headers: Arc<AuthHeaders>,
    pub retry_opts: RetryOpts,
    pub requester: Arc<str>,
    pub offline: bool,
    pub verify_store_integrity: bool,
    pub package_id: String,
    pub package_url: String,
    pub integrity: Integrity,
    pub package_unpacked_size: Option<usize>,
    pub package_file_count: Option<usize>,
}

/// [`tokio::spawn`] a single tarball download into the shared mem cache
/// and store. The task's result is discarded — the [`MemCache`] carries
/// `CacheValue::Available` (success) or `CacheValue::Failed` (error) to
/// the install pass that later looks up the same URL. The download is
/// routed through [`SilentReporter`]: the pnpr client's frozen
/// materialization install emits the `resolved → fetched/found_in_store
/// → imported` progress itself as it consumes each tarball, so the
/// prefetch must not emit a competing, out-of-order set.
pub(crate) fn spawn_tarball_download(download: TarballDownload) {
    let TarballDownload {
        http_client,
        mem_cache,
        store_dir,
        store_index,
        store_index_writer,
        verified_files_cache,
        auth_headers,
        retry_opts,
        requester,
        offline,
        verify_store_integrity,
        package_id,
        package_url,
        integrity,
        package_unpacked_size,
        package_file_count,
    } = download;

    tokio::spawn(async move {
        let _ = DownloadTarballToStore {
            http_client: &http_client,
            store_dir,
            store_index,
            store_index_writer,
            verify_store_integrity,
            verified_files_cache,
            package_integrity: &integrity,
            package_unpacked_size,
            package_file_count,
            package_url: &package_url,
            package_id: &package_id,
            requester: &requester,
            prefetched_cas_paths: None,
            retry_opts,
            auth_headers: &auth_headers,
            ignore_file_pattern: None,
            offline,
            // The client prefetch routes through `SilentReporter`, so
            // there's no install reporter to dedup progress events
            // against — the frozen materialization install emits its own
            // progress as it consumes each tarball from the mem cache.
            progress_reported: None,
        }
        .run_with_mem_cache::<SilentReporter>(&mem_cache)
        .await;
    });
}

/// Fires background tarball downloads on the pnpr client as resolved
/// packages stream in from `/v1/resolve`, so each tarball fetch overlaps
/// the server's still-running resolution rather than waiting for the
/// finished lockfile.
///
/// Mirrors the local fresh-install [`crate::PrefetchingResolver`] —
/// each download lands in the shared [`MemCache`] keyed by tarball URL,
/// and the frozen materialization install the client runs afterward
/// picks it up from the cache. It carries its own store-index writer so
/// freshly-downloaded tarballs are recorded in `index.db` (the frozen
/// install hits the mem cache and never writes the row itself), matching
/// how `PrefetchingResolver` shares the install's writer.
#[must_use]
pub struct TarballPrefetcher {
    http_client: Arc<ThrottledClient>,
    mem_cache: Arc<MemCache>,
    store_dir: &'static StoreDir,
    store_index: Option<SharedReadonlyStoreIndex>,
    store_index_writer: Arc<StoreIndexWriter>,
    writer_task: tokio::task::JoinHandle<Result<(), StoreIndexError>>,
    verified_files_cache: SharedVerifiedFilesCache,
    auth_headers: Arc<AuthHeaders>,
    retry_opts: RetryOpts,
    requester: Arc<str>,
    offline: bool,
    verify_store_integrity: bool,
    /// URLs already spawned, so repeated frames for the same tarball
    /// (the resolver yields one per dependent edge) collapse to a single
    /// download. Mirrors `PrefetchingResolver::spawned_urls`.
    spawned_urls: DashSet<String>,
}

impl TarballPrefetcher {
    /// Build a prefetcher bound to the install's shared mem cache and
    /// HTTP client. Opens the store index read-only (best-effort: a
    /// missing `index.db` just means every prefetch falls through to a
    /// network fetch) and spawns a batched store-index writer for the
    /// freshly-downloaded rows.
    pub async fn new(
        config: &'static Config,
        http_client: &Arc<ThrottledClient>,
        mem_cache: &Arc<MemCache>,
        auth_override: Option<&Arc<AuthHeaders>>,
        requester: &str,
    ) -> Self {
        let store_dir = &config.store_dir;
        let store_index = {
            let store_dir = store_dir.clone();
            match tokio::task::spawn_blocking(move || StoreIndex::shared_readonly_in(&store_dir))
                .await
            {
                Ok(store_index) => store_index,
                Err(error) => {
                    tracing::warn!(
                        target: "pacquet::pnpr",
                        ?error,
                        "store-index open task failed; prefetching without a shared cache index",
                    );
                    None
                }
            }
        };
        let (store_index_writer, writer_task) = StoreIndexWriter::spawn(store_dir);
        let auth_headers =
            auth_override.map_or_else(|| Arc::clone(&config.auth_headers), Arc::clone);
        TarballPrefetcher {
            http_client: Arc::clone(http_client),
            mem_cache: Arc::clone(mem_cache),
            store_dir,
            store_index,
            store_index_writer,
            writer_task,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            auth_headers,
            retry_opts: retry_opts_from_config(config),
            requester: Arc::<str>::from(requester),
            offline: config.offline,
            verify_store_integrity: config.verify_store_integrity,
            spawned_urls: DashSet::new(),
        }
    }

    /// Fire a background download of one resolved tarball. Deduplicated
    /// by URL; a no-op when the same URL was already prefetched or when
    /// `integrity` doesn't parse (the materialization install fetches
    /// that package the normal way). `unpacked_size` (the frame's
    /// `unpackedSize`, when the registry published one) sizes the
    /// decompression buffer and acts as the download's queueing
    /// priority — largest pending archives start first.
    pub fn prefetch(
        &self,
        package_id: String,
        package_url: String,
        integrity: &str,
        unpacked_size: Option<usize>,
        file_count: Option<usize>,
    ) {
        let integrity = match integrity.parse::<Integrity>() {
            Ok(integrity) => integrity,
            Err(error) => {
                tracing::debug!(
                    target: "pacquet::pnpr",
                    %package_url,
                    ?error,
                    "skipping tarball prefetch: unparsable integrity",
                );
                return;
            }
        };
        if !self.spawned_urls.insert(package_url.clone()) {
            return;
        }
        spawn_tarball_download(TarballDownload {
            http_client: Arc::clone(&self.http_client),
            mem_cache: Arc::clone(&self.mem_cache),
            store_dir: self.store_dir,
            store_index: self.store_index.clone(),
            store_index_writer: Some(Arc::clone(&self.store_index_writer)),
            verified_files_cache: SharedVerifiedFilesCache::clone(&self.verified_files_cache),
            auth_headers: Arc::clone(&self.auth_headers),
            retry_opts: self.retry_opts,
            requester: Arc::clone(&self.requester),
            offline: self.offline,
            verify_store_integrity: self.verify_store_integrity,
            package_id,
            package_url,
            integrity,
            package_unpacked_size: unpacked_size,
            package_file_count: file_count,
        });
    }

    /// Drain the store-index writer. Call after the materialization
    /// install has returned: by then every prefetch download has
    /// finished (the install awaited each tarball's mem-cache slot) and
    /// queued its index row, so dropping the writer handle closes the
    /// channel and this awaits the final batch flush. A writer error is
    /// downgraded to a warning — the install already succeeded and a
    /// missing index row only costs the next install a re-download.
    pub async fn shutdown(self) {
        drop(self.store_index_writer);
        match self.writer_task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(
                target: "pacquet::pnpr",
                ?error,
                "store-index writer task returned an error; some rows may not be persisted",
            ),
            Err(error) => tracing::warn!(
                target: "pacquet::pnpr",
                ?error,
                "store-index writer task panicked; some rows may not be persisted",
            ),
        }
    }
}
