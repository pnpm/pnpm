//! The tarball downloads a resolver prefetch spawned for a run that fetches
//! into the store. A speculative prefetch spawns its downloads detached and
//! reports through the mem cache, where the materialization that follows
//! finds them. A run that materializes nothing has no such pass, so it keeps
//! the download tasks themselves, waits for them, and hears their failures.

use pnpm_tarball::TarballError;
use std::sync::{Mutex, PoisonError};
use tokio::task::JoinHandle;

/// The result of one tracked download.
pub type DownloadOutcome = Result<(), TarballError>;

#[derive(Default)]
pub struct PrefetchDownloads {
    handles: Mutex<Vec<(String, JoinHandle<DownloadOutcome>)>>,
}

impl PrefetchDownloads {
    /// Keep a spawned download, under its tarball URL.
    pub fn track(&self, package_url: String, handle: JoinHandle<DownloadOutcome>) {
        self.handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((package_url, handle));
    }

    /// Wait for every download kept so far; the failures, by tarball URL.
    /// A download task that panicked propagates its panic.
    pub async fn wait(&self) -> Vec<(String, TarballError)> {
        let mut failed = Vec::new();
        loop {
            let batch =
                std::mem::take(&mut *self.handles.lock().unwrap_or_else(PoisonError::into_inner));
            if batch.is_empty() {
                return failed;
            }
            for (package_url, handle) in batch {
                if let Err(error) = handle.await.expect("tarball download task panicked") {
                    failed.push((package_url, error));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
