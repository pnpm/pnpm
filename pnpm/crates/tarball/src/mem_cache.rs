use std::{collections::BTreeMap, sync::Arc};

use dashmap::DashMap;
use pipe_trait::Pipe;
use pnpm_package_manifest::PackageBin;
use tokio::sync::{Notify, RwLock};

use crate::TarballError;

/// A settled mem-cache slot: the extracted CAS map, the bundled
/// manifest, and the build-trigger verdict the extraction recorded. The
/// install pass reads the map and verdict; a resolve-time archive read
/// parks on the same slot for the manifest.
#[derive(Debug, Clone)]
pub struct CachedTarball {
    pub files: Arc<pnpm_fs::CasPaths>,
    pub manifest: Option<serde_json::Value>,
    pub requires_build: Option<bool>,
    pub bins: Option<Arc<BTreeMap<String, PackageBin>>>,
}

impl CachedTarball {
    /// A settled slot that carries only the CAS map, for callers that
    /// reconstructed the files without a bundled manifest.
    #[must_use]
    pub fn from_files(files: pnpm_fs::CasPaths) -> Arc<Self> {
        Self::from_files_arc(Arc::new(files))
    }

    #[must_use]
    pub fn from_files_arc(files: Arc<pnpm_fs::CasPaths>) -> Arc<Self> {
        Arc::new(Self { files, manifest: None, requires_build: None, bins: None })
    }

    /// Whether the extracted archive carries build triggers. Uses the verdict
    /// recorded during extraction or store-index load when available, falling
    /// back to the in-memory bundled manifest or the CAS file map.
    #[must_use]
    pub fn requires_build(&self) -> bool {
        self.requires_build.unwrap_or_else(|| {
            if let Some(manifest) = self.manifest.as_ref() {
                let mut triggers = pnpm_package_manifest::files_build_triggers(self.files.keys());
                triggers.read_manifest(manifest);
                triggers.requires_build()
            } else {
                pnpm_package_manifest::requires_build_from_cas_paths(&self.files)
            }
        })
    }
}

/// Value of the cache.
#[derive(Debug, Clone)]
pub enum CacheValue {
    /// The package is being processed.
    InProgress(Arc<Notify>),
    /// The package is saved.
    Available(Arc<CachedTarball>),
    /// The owning fetch failed with this rendered error, after its retries
    /// ran out or on an error it does not retry. The slot stays failed for
    /// the life of the cache, so every other request for the archive reports
    /// [`TarballError::SiblingFetchFailed`] instead of spending another retry
    /// budget. The originating [`TarballError`] is not `Clone`.
    Failed(Arc<str>),
}

/// Internal in-memory cache of tarballs.
///
/// Ordinary package entries retain their tarball URL key. Revision-addressed
/// fetches and projections that produce different file sets add a discriminator
/// so incompatible network policies or archive views never share a result.
pub type MemCache = DashMap<String, Arc<RwLock<CacheValue>>>;

pub(crate) enum CacheSlotClaim<'a> {
    Claimed(CacheClaimGuard<'a>),
    Existing(Arc<RwLock<CacheValue>>),
}

/// Owns an `InProgress` slot in [`MemCache`] until the fetch settles it.
///
/// Dropping the guard before [`Self::publish_available`] or
/// [`Self::publish_failed`] removes the unsettled slot and wakes any
/// parked waiters so they can claim a fresh attempt instead of hanging.
pub(crate) struct CacheClaimGuard<'a> {
    mem_cache: &'a MemCache,
    mem_cache_key: String,
    cache_lock: Arc<RwLock<CacheValue>>,
    notify: Arc<Notify>,
    settled: bool,
}

impl CacheClaimGuard<'_> {
    pub(crate) async fn publish_available(mut self, cached: CachedTarball) -> Arc<CachedTarball> {
        let cached = Arc::new(cached);
        *self.cache_lock.write().await = CacheValue::Available(Arc::clone(&cached));
        self.settled = true;
        self.notify.notify_waiters();
        cached
    }

    pub(crate) async fn publish_failed(mut self, error: &TarballError) {
        *self.cache_lock.write().await = CacheValue::Failed(error.to_string().into());
        self.settled = true;
        self.notify.notify_waiters();
    }
}

impl Drop for CacheClaimGuard<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        self.mem_cache.remove_if(&self.mem_cache_key, |_, current| {
            Arc::ptr_eq(current, &self.cache_lock)
        });
        self.notify.notify_waiters();
    }
}

/// Claim the single fetch for this key. Releases the entry guard before
/// the caller awaits either the cache lock or the owning fetch.
pub(crate) fn claim_cache_entry<'a>(
    mem_cache: &'a MemCache,
    mem_cache_key: &str,
) -> CacheSlotClaim<'a> {
    match mem_cache.entry(mem_cache_key.to_owned()) {
        dashmap::mapref::entry::Entry::Occupied(entry) => {
            CacheSlotClaim::Existing(Arc::clone(entry.get()))
        }
        dashmap::mapref::entry::Entry::Vacant(entry) => {
            let notify = Arc::new(Notify::new());
            let cache_lock = notify
                .pipe_ref(Arc::clone)
                .pipe(CacheValue::InProgress)
                .pipe(RwLock::new)
                .pipe(Arc::new);
            entry.insert(Arc::clone(&cache_lock));
            CacheSlotClaim::Claimed(CacheClaimGuard {
                mem_cache,
                mem_cache_key: mem_cache_key.to_owned(),
                cache_lock,
                notify,
                settled: false,
            })
        }
    }
}

/// Park until the task that claimed this key settles the cache slot, or
/// return `Ok(None)` when the owning task was dropped without settling it.
///
/// Register with the `Notify` *before* re-checking the slot.
/// `notify_waiters` stores no permit — it wakes only `Notified`
/// futures already registered at that instant — and the read guard
/// from an `InProgress` observation is released before the loop,
/// so the owner's flip-and-notify can land in between. Checking
/// first and registering after loses that wakeup and parks this
/// task forever (nothing ever notifies the slot again once it is
/// terminal).
pub(crate) async fn wait_for_cached_tarball(
    mem_cache: &MemCache,
    mem_cache_key: &str,
    cache_lock: &Arc<RwLock<CacheValue>>,
    package_url: &str,
) -> Result<Option<Arc<CachedTarball>>, TarballError> {
    // Read-lock the state: the variant inspection doesn't mutate
    // anything, and a `write().await` would serialize every late
    // visitor for a popular tarball behind a single exclusive guard.
    // The owner branch is the only writer; the RwLock's reader-writer
    // fairness guarantees the owner still makes progress.
    let notify = match &*cache_lock.read().await {
        CacheValue::Available(cached) => return Ok(Some(Arc::clone(cached))),
        CacheValue::InProgress(notify) => Arc::clone(notify),
        CacheValue::Failed(cause) => return Err(sibling_fetch_failed(cause)),
    };

    tracing::info!(target: "pacquet::download", package_url = ?package_url, "Wait for cache");
    loop {
        let notified = notify.notified();
        let mut notified = std::pin::pin!(notified);
        notified.as_mut().enable();
        match &*cache_lock.read().await {
            CacheValue::Available(cached) => return Ok(Some(Arc::clone(cached))),
            CacheValue::Failed(cause) => return Err(sibling_fetch_failed(cause)),
            CacheValue::InProgress(_) => {
                let still_claimed = mem_cache
                    .get(mem_cache_key)
                    .is_some_and(|entry| Arc::ptr_eq(entry.value(), cache_lock));
                if !still_claimed {
                    return Ok(None);
                }
            }
        }
        notified.await;
    }
}

fn sibling_fetch_failed(cause: &Arc<str>) -> TarballError {
    TarballError::SiblingFetchFailed { cause: Arc::clone(cause) }
}
