//! The store verification a run that fetches into the store starts while
//! it resolves. See [`StoreVerification`].

use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir};
use pnpm_tarball::{PrefetchIntegrityCheck, PrefetchResult, prefetch_cas_paths};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::task::JoinHandle;

/// The verified store rows of the wanted lockfile's packages, read and
/// checked while the resolution runs.
///
/// A fresh resolution reuses most of a lockfile it resolves against, and
/// the packages it reuses never go through the resolver's prefetch, so a
/// run that fetches into the store used to verify them only after the
/// resolution: every row read again, every file stat'ed again, with
/// nothing to overlap. The verification now starts as soon as the store
/// index is open, against the verified-files cache the resolution shares,
/// and [`Self::finish`] hands its rows to the fetch that follows.
#[derive(Default)]
pub(crate) struct StoreVerification {
    started: Mutex<Option<Started>>,
}

struct Started {
    task: JoinHandle<PrefetchResult>,
    files: SharedVerifiedFilesCache,
}

/// What [`StoreVerification::finish`] hands to the store fetch.
pub(crate) struct StoreVerified {
    /// The rows of the packages the store holds with their files in place.
    pub(crate) rows: Arc<PrefetchResult>,
    /// The files those rows were verified against, so the fetch does not
    /// stat them again.
    pub(crate) files: SharedVerifiedFilesCache,
}

#[cfg(test)]
mod tests;

impl StoreVerification {
    /// Start verifying the rows of `keys`, store-index keys of the wanted
    /// lockfile's registry packages. A second start replaces the first.
    pub(crate) fn start(
        &self,
        keys: Vec<String>,
        index: &SharedReadonlyStoreIndex,
        store_dir: &'static StoreDir,
        verify_integrity: bool,
        files: &SharedVerifiedFilesCache,
    ) {
        let task = tokio::spawn(prefetch_cas_paths(
            Some(Arc::clone(index)),
            store_dir,
            keys,
            PrefetchIntegrityCheck::eager_if(verify_integrity),
            Arc::clone(files),
        ));
        *self.started.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Started { task, files: Arc::clone(files) });
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
            Ok(rows) => Some(StoreVerified { rows: Arc::new(rows), files: started.files }),
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
