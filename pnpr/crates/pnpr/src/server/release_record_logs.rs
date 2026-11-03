//! Integrating the transparency logs of hosted release records
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#transparency-log)).
//!
//! A record's log entry is sequenced when its version is accepted. A commit
//! that sequenced entries integrates their log in the background, and
//! startup integrates every log, which picks up whatever an interrupted
//! integration left. A publication's success means committed, not yet
//! integrated. Without a log signing key, entries are sequenced and nothing
//! is integrated.

use super::AppState;
use pnpr_config::Config;
use pnpr_storage::{Storage, transparency_log::LogId};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Integrate `log` in the background, after a commit that sequenced entries
/// into it.
pub(super) fn integrate_in_background(state: &AppState, log: LogId) {
    let Some(key) = state.inner.config.release_records.log_signing_key.clone() else {
        return;
    };
    let storage = state.inner.storage.clone();
    let lock = Arc::clone(&state.inner.locks.log_integration);
    tokio::spawn(async move {
        let _integrating = lock.lock().await;
        if let Err(error) = storage.integrate_log(&log, &key).await {
            tracing::warn!(%error, %log, "integrating a transparency log failed");
        }
    });
}

/// Integrate, in the background, every log whose entries were sequenced,
/// as startup does after an unclean shutdown.
pub(crate) fn integrate_sequenced_logs(config: &Config) -> pnpr_error::Result<()> {
    let Some(key) = config.release_records.log_signing_key.clone() else {
        return Ok(());
    };
    let storage = Storage::from_config(&config.storage)?;
    let Some(database) = storage.sql_metadata().cloned() else {
        return Ok(());
    };
    let lock = Arc::new(Mutex::new(()));
    tokio::spawn(async move {
        let _integrating = lock.lock().await;
        let logs = match database.sequenced_logs().await {
            Ok(logs) => logs,
            Err(error) => {
                tracing::warn!(%error, "listing the transparency logs to integrate failed");
                return;
            }
        };
        for (log, _) in logs {
            if let Err(error) = storage.integrate_log(&log, &key).await {
                tracing::warn!(%error, %log, "integrating a transparency log failed");
            }
        }
    });
    Ok(())
}
