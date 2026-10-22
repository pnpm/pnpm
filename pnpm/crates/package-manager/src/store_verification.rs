//! The store verification a run that fetches into the store starts while
//! it resolves. See [`StoreVerification`].

use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir};
use pnpm_tarball::{PrefetchIntegrityCheck, prefetch_cas_paths};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::task::JoinHandle;

/// Store-index keys read and checked per batch. The shared index is locked
/// for a batch's read, so the resolver's own lookups get in between
/// batches, and a stopped verification ends at the next batch boundary.
const KEYS_PER_BATCH: usize = 256;

/// The store-index keys of the wanted lockfile's packages whose files the
/// store holds intact, checked while the resolution runs.
///
/// A fresh resolution reuses most of the lockfile it resolves against, and
/// the packages it reuses never go through the resolver's prefetch. A run
/// that fetches into the store checks them here from the moment the store
/// index is open, against the verified-files cache the resolution shares.
/// [`Self::finish`] hands the keys to the fetch that follows, which then
/// verifies only the rows the resolution added.
#[derive(Default)]
pub(crate) struct StoreVerification {
    started: Mutex<Option<Started>>,
}

struct Started {
    task: JoinHandle<VerifiedStoreKeys>,
    files: SharedVerifiedFilesCache,
}

/// The store-index keys of rows whose files are in place.
pub(crate) type VerifiedStoreKeys = HashSet<String>;

/// What [`StoreVerification::finish`] hands to the store fetch.
pub(crate) struct StoreVerified {
    /// The keys of the packages the store holds with their files in place.
    pub(crate) keys: Arc<VerifiedStoreKeys>,
    /// The files those rows were verified against, so the fetch does not
    /// stat them again.
    pub(crate) files: SharedVerifiedFilesCache,
}

#[cfg(test)]
mod tests;

impl StoreVerification {
    /// Start verifying the rows of `keys`, store-index keys of the wanted
    /// lockfile's registry packages. A verification started earlier is
    /// stopped.
    pub(crate) fn start(
        &self,
        keys: Vec<String>,
        index: &SharedReadonlyStoreIndex,
        store_dir: &'static StoreDir,
        verify_integrity: bool,
        files: &SharedVerifiedFilesCache,
    ) {
        self.start_in_batches(keys, index, store_dir, verify_integrity, files, KEYS_PER_BATCH);
    }

    /// [`Self::start`] with `batch` keys per store read.
    fn start_in_batches(
        &self,
        keys: Vec<String>,
        index: &SharedReadonlyStoreIndex,
        store_dir: &'static StoreDir,
        verify_integrity: bool,
        files: &SharedVerifiedFilesCache,
        batch: usize,
    ) {
        let task = tokio::spawn(verify_in_batches(
            Arc::clone(index),
            store_dir,
            keys,
            PrefetchIntegrityCheck::eager_if(verify_integrity),
            Arc::clone(files),
            batch,
        ));
        let previous = self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(Started { task, files: Arc::clone(files) });
        if let Some(previous) = previous {
            previous.task.abort();
        }
    }

    /// Wait for the verification started here, if one was. A task that
    /// panicked or was cancelled counts as no verification: the fetch then
    /// reads the rows itself.
    pub(crate) async fn finish(&self) -> Option<StoreVerified> {
        let started = self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()?;
        match started.task.await {
            Ok(keys) => Some(StoreVerified { keys: Arc::new(keys), files: started.files }),
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "early store verification failed; the store fetch verifies the rows itself",
                );
                None
            }
        }
    }
}

/// A run that fails before its fetch drops the verification: the task
/// stops at its next batch instead of reading the rest of the store for
/// nobody.
impl Drop for StoreVerification {
    fn drop(&mut self) {
        if let Some(started) = self.started.get_mut().unwrap_or_else(PoisonError::into_inner) {
            started.task.abort();
        }
    }
}

/// Verify `keys` a batch at a time. Each batch is one blocking store read
/// and check; between batches the task yields, which is where an abort
/// takes effect and where the shared index is unlocked.
async fn verify_in_batches(
    index: SharedReadonlyStoreIndex,
    store_dir: &'static StoreDir,
    keys: Vec<String>,
    check: PrefetchIntegrityCheck,
    files: SharedVerifiedFilesCache,
    batch: usize,
) -> VerifiedStoreKeys {
    let mut verified = VerifiedStoreKeys::new();
    let mut keys = keys.into_iter();
    loop {
        let batch_keys: Vec<String> = keys
            .by_ref()
            .take(batch.max(1))
            .collect();
        if batch_keys.is_empty() {
            return verified;
        }
        let result = prefetch_cas_paths(
            Some(Arc::clone(&index)),
            store_dir,
            batch_keys,
            check,
            Arc::clone(&files),
        )
        .await;
        verified.extend(result.cas_paths.into_keys());
    }
}
