//! Integrating the transparency logs of hosted release records
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#transparency-log)).
//!
//! A record's log entry is sequenced when its version is accepted. A commit
//! that sequenced entries integrates their log in the background, and each
//! server once integrates every log, which picks up whatever an interrupted
//! integration left: when its router is built, or, for a router built
//! outside a Tokio runtime, at the first integration it starts or the
//! first read of a log. A publication's success means committed, not yet
//! integrated. Without a log signing key, entries are sequenced and nothing
//! is integrated.

use super::AppState;
use pnpr_storage::transparency_log::LogId;
use std::sync::{Arc, atomic::Ordering};

/// Integrate `log` in the background, after a commit that sequenced entries
/// into it.
pub(super) fn integrate_in_background(state: &AppState, log: LogId) {
    let Some(key) = state.inner.config.release_records.log_signing_key.clone() else {
        return;
    };
    integrate_sequenced_logs(state);
    let state = state.clone();
    tokio::spawn(async move {
        let _integrating = state.inner.locks.log_integration.lock().await;
        if let Err(error) = state.inner.storage.integrate_log(&log, &key).await {
            tracing::warn!(%error, %log, "integrating a transparency log failed");
            // The next trigger scans every log again, this one included.
            state.inner.locks.logs_recovered.store(false, Ordering::Release);
        }
    });
}

/// Integrate, in the background, every log whose entries were sequenced,
/// once per server, as after an unclean shutdown. Outside a Tokio runtime it
/// does nothing and leaves the scan to the next call. It shares the server's
/// storage and its integration lock with the integrations commits start, so
/// one process never integrates a log twice at once: the filesystem
/// backend's compare-and-set of a checkpoint holds only within one store.
pub(super) fn integrate_sequenced_logs(state: &AppState) {
    let Some(key) = state.inner.config.release_records.log_signing_key.clone() else {
        return;
    };
    let storage = state.inner.storage.clone();
    let database = Arc::clone(storage.sql_metadata());
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    if state.inner.locks.logs_recovered.swap(true, Ordering::AcqRel) {
        return;
    }
    // A scan that fails or never finishes, because its runtime shut down,
    // is retried by the next call.
    let mut retry = RetryRecovery(Some(state.clone()));
    let lock = Arc::clone(&state.inner.locks.log_integration);
    runtime.spawn(async move {
        let _integrating = lock.lock().await;
        if integrate_every_log(&storage, &database, &key).await {
            retry.disarm();
        }
    });
}

/// Clears a server's `logs_recovered` when dropped holding it, so a
/// recovery scan that failed or was cancelled runs again.
struct RetryRecovery(Option<AppState>);

impl RetryRecovery {
    /// Keep `logs_recovered` set: the scan succeeded.
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for RetryRecovery {
    fn drop(&mut self) {
        if let Some(state) = &self.0 {
            state.inner.locks.logs_recovered.store(false, Ordering::Release);
        }
    }
}

/// Integrate every sequenced log, reporting whether all of them were.
async fn integrate_every_log(
    storage: &pnpr_storage::Storage,
    database: &Arc<pnpr_storage::metadata::SqlMetadata>,
    key: &pnpm_transparency_log::NoteSigner,
) -> bool {
    let logs = match database.sequenced_logs().await {
        Ok(logs) => logs,
        Err(error) => {
            tracing::warn!(%error, "listing the transparency logs to integrate failed");
            return false;
        }
    };
    let mut integrated = true;
    for (log, _) in logs {
        if let Err(error) = storage.integrate_log(&log, key).await {
            tracing::warn!(%error, %log, "integrating a transparency log failed");
            integrated = false;
        }
    }
    integrated
}
