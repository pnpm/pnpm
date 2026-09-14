//! Background tarball downloads for the pnpr client path.
//!
//! [`TarballPrefetcher`] fires a download as each `package` frame streams
//! in from `/-/pnpr/v0/resolve` so the fetch overlaps the *server's* resolution.
//! It is the streaming-client analogue of [`crate::PrefetchingResolver`] (the
//! local fresh-install prefetcher), but independent: the resolver path
//! reports prefetch progress through the install reporter, whereas this
//! one runs silently and lets the frozen materialization install emit
//! progress as it consumes each tarball.
//!
//! Each download lands its result in the shared [`MemCache`] keyed by
//! tarball URL and fetch policy; the later install pass picks it up via
//! [`IngestTarballToStore::run_with_mem_cache`] (an immediate
//! `CacheValue::Available` hit, or a brief park on the per-URL `Notify`
//! while the prefetch finishes).

use crate::{
    install_package_by_snapshot::tarball_url_and_integrity, retry_config::retry_opts_from_config,
};
use dashmap::DashSet;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndex, StoreIndexError,
    StoreIndexWriter, store_index_key,
};
use pnpm_tarball::{IngestTarballToStore, MemCache, RetryOpts, TarballError};
use ssri::Integrity;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

/// One registry lockfile entry [`TarballPrefetcher::prefetch_lockfile`]
/// may spawn a download for, staged so the whole batch can be filtered
/// through a single store-index existence probe first.
struct PendingPrefetch {
    store_key: String,
    package_id: String,
    package_url: String,
    integrity: String,
    revision_addressed: bool,
}

/// Drop every pending entry whose `(integrity, package_id)` row already
/// exists in `index.db`, with one batched existence probe.
async fn without_store_hits(
    index: Option<SharedReadonlyStoreIndex>,
    pending: Vec<PendingPrefetch>,
) -> Vec<PendingPrefetch> {
    let Some(index) = index else {
        return pending;
    };
    let keys: Vec<String> = pending
        .iter()
        .map(|entry| entry.store_key.clone())
        .collect();
    let hits = tokio::task::spawn_blocking(move || {
        let Ok(guard) = index.lock() else {
            return HashSet::new();
        };
        guard.contains_many(&keys).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    pending
        .into_iter()
        .filter(|entry| !hits.contains(&entry.store_key))
        .collect()
}

/// One background tarball download. Every field is owned (an `Arc`
/// clone or a `Copy` scalar) so the spawned task captures an
/// independent set without borrowing the caller's scope.
pub(crate) struct TarballDownload {
    pub mem_cache: Arc<MemCache>,
    pub requester: Arc<str>,
    pub store: pnpm_tarball::ArchiveStoreContext<'static>,
    pub fetching: crate::tarball_prefetch::PrefetchHttpClient,
    pub package: crate::tarball_prefetch::TarballDownloadPackage,
}

#[derive(Clone)]
pub(crate) struct PrefetchHttpClient {
    pub http_client: Arc<ThrottledClient>,
    pub auth_headers: Arc<AuthHeaders>,
    pub retry_opts: RetryOpts,
    pub offline: bool,
}

pub(crate) struct TarballDownloadPackage {
    pub id: String,
    pub url: String,
    pub integrity: Integrity,
    pub unpacked_size: Option<usize>,
    pub file_count: Option<usize>,
    pub revision_addressed: bool,
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
    tokio::spawn(async move {
        let _ = run_tarball_download(download).await;
    });
}

async fn run_tarball_download(
    download: TarballDownload,
) -> Result<Arc<HashMap<String, PathBuf>>, TarballError> {
    let ingest = IngestTarballToStore {
        fetching: download.fetching.options(),
        package: pnpm_tarball::TarballPackage {
            integrity: Some(&download.package.integrity),
            unpacked_size: download.package.unpacked_size,
            file_count: download.package.file_count,
            url: &download.package.url,
            id: &download.package.id,
        },
        store: download.store,

        requester: &download.requester,

        ignore_file_pattern: None,

        // The client prefetch routes through `SilentReporter`, so
        // there's no install reporter to dedup progress events
        // against — the frozen materialization install emits its own
        // progress as it consumes each tarball from the mem cache.
        progress_reported: None,
        store_projection: pnpm_tarball::ArchiveStoreProjection::Package { append_manifest: None },
    };
    if download.package.revision_addressed {
        ingest.run_revision_addressed_with_mem_cache::<SilentReporter>(&download.mem_cache).await
    } else {
        ingest.run_with_mem_cache::<SilentReporter>(&download.mem_cache).await
    }
}

/// Fires background tarball downloads on the pnpr client as resolved
/// packages stream in from `/-/pnpr/v0/resolve`, so each tarball fetch overlaps
/// the server's still-running resolution rather than waiting for the
/// finished lockfile.
///
/// Mirrors the local fresh-install [`crate::PrefetchingResolver`] —
/// each download lands in the shared [`MemCache`] keyed by tarball URL and
/// fetch policy,
/// and the frozen materialization install the client runs afterward
/// picks it up from the cache. It carries its own store-index writer so
/// freshly-downloaded tarballs are recorded in `index.db` (the frozen
/// install hits the mem cache and never writes the row itself), matching
/// how `PrefetchingResolver` shares the install's writer.
#[must_use]
pub struct TarballPrefetcher {
    mem_cache: Arc<MemCache>,
    writer_task: tokio::task::JoinHandle<Result<(), StoreIndexError>>,
    requester: Arc<str>,
    /// URLs already spawned, so repeated frames for the same tarball
    /// (the resolver yields one per dependent edge) collapse to a single
    /// download. Mirrors `PrefetchingResolver::spawned_urls`.
    spawned_urls: DashSet<String>,
    store: pnpm_tarball::ArchiveStoreContext<'static>,
    fetching: crate::tarball_prefetch::PrefetchHttpClient,
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
        TarballPrefetcher {
            mem_cache: Arc::clone(mem_cache),
            writer_task,
            requester: Arc::<str>::from(requester),
            spawned_urls: DashSet::new(),
            store: pnpm_tarball::ArchiveStoreContext {
                dir: store_dir,
                index: store_index,
                index_writer: Some(store_index_writer),
                verified_files_cache: SharedVerifiedFilesCache::default(),
                verify_integrity: config.verify_store_integrity,
                strict_pkg_content_check: config.strict_store_pkg_content_check,
                prefetched_cas_paths: None,
            },
            fetching: PrefetchHttpClient::new(config, http_client, auth_override),
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
        revision_addressed: bool,
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
            mem_cache: Arc::clone(&self.mem_cache),
            requester: Arc::clone(&self.requester),
            store: self.store.clone(),
            fetching: self.fetching.clone(),
            package: crate::tarball_prefetch::TarballDownloadPackage {
                id: package_id,
                url: package_url,
                integrity,
                unpacked_size,
                file_count,
                revision_addressed,
            },
        });
    }

    /// Fire a background download for every registry-resolved entry of a
    /// frozen lockfile that isn't already in the store, so the fetch
    /// starts before the trust verdict arrives. Registry resolutions
    /// only: they are the one shape the materialization pass reuses from
    /// the shared mem cache (see the dispatch in
    /// [`crate::InstallPackageBySnapshot`]).
    ///
    /// Entries with an `index.db` row are filtered out with one batched
    /// existence probe rather than spawned: the materialization pass
    /// already covers warm entries with its own batched verified lookup
    /// ([`pnpm_tarball::prefetch_cas_paths`]), so spawning them here
    /// would only duplicate that work per key — on a fully warm store it
    /// turns the whole prefetch into a no-op. A row whose CAS files have
    /// gone missing is skipped here too; the materialization pass's
    /// per-snapshot cache-miss fallback re-downloads it.
    pub async fn prefetch_lockfile(&self, lockfile: &Lockfile, config: &Config) {
        let Some(packages) = lockfile.packages.as_ref() else {
            return;
        };
        let mut pending = Vec::with_capacity(packages.len());
        for (package_key, metadata) in packages {
            if !matches!(&metadata.resolution, LockfileResolution::Registry(_)) {
                continue;
            }
            let (tarball_url, integrity) =
                tarball_url_and_integrity(&metadata.resolution, package_key, config)
                    .expect("registry resolutions are always fetchable");
            let package_id = package_key.pkg_id();
            let integrity =
                integrity.expect("registry resolutions always carry an integrity").to_string();
            let revision_addressed = matches!(
                &metadata.resolution,
                LockfileResolution::Registry(registry) if registry.revision.is_some(),
            );
            pending.push(PendingPrefetch {
                store_key: store_index_key(&integrity, &package_id),
                package_id,
                package_url: tarball_url.into_owned(),
                integrity,
                revision_addressed,
            });
        }
        for entry in without_store_hits(self.store.index.clone(), pending).await {
            let PendingPrefetch {
                package_id,
                package_url,
                integrity,
                revision_addressed,
                ..
            } = entry;
            // The lockfile records no dist size hints, so the downloads
            // queue without a work estimate.
            self.prefetch(package_id, package_url, &integrity, None, None, revision_addressed);
        }
    }

    /// Drain the store-index writer. Call after the materialization
    /// install has returned: by then every prefetch download has
    /// finished (the install awaited each tarball's mem-cache slot) and
    /// queued its index row, so dropping the writer handle closes the
    /// channel and this awaits the final batch flush. A writer error is
    /// downgraded to a warning — the install already succeeded and a
    /// missing index row only costs the next install a re-download.
    pub async fn shutdown(self) {
        drop(self.store.index_writer);
        StoreIndexWriter::drain(self.writer_task, "; some rows may not be persisted").await;
    }
}

#[cfg(test)]
mod tests;

impl PrefetchHttpClient {
    pub(crate) fn new(
        config: &Config,
        http_client: &Arc<ThrottledClient>,
        auth_override: Option<&Arc<AuthHeaders>>,
    ) -> Self {
        Self {
            http_client: Arc::clone(http_client),
            auth_headers: auth_override.map_or_else(
                || Arc::clone(&config.auth_headers),
                Arc::clone,
            ),
            retry_opts: retry_opts_from_config(config),
            offline: config.offline,
        }
    }

    pub(crate) fn options(&self) -> pnpm_tarball::ArchiveFetchOptions<'_> {
        pnpm_tarball::ArchiveFetchOptions {
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            retry_opts: self.retry_opts,
            offline: self.offline,
        }
    }
}
