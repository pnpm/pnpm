//! Integrating and reconciling the transparency logs of hosted release records
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#transparency-log)).
//!
//! A record's log entry is sequenced when its version is accepted. A commit
//! that sequenced entries integrates their log in the background, and each
//! server once integrates every log, which picks up whatever an interrupted
//! integration left: when its router is built, or, for a router built
//! outside a Tokio runtime, at the first integration it starts or the
//! first read of a log. Startup and publish commits reconcile each log against
//! `.transparency-log/` before new entries are sequenced, re-importing entries
//! from stored bundles after a metadata rollback and refusing forked logs.

use super::AppState;
use pnpm_transparency_log::NoteSigner;
use pnpr_config::Config;
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use pnpr_storage::{Storage, metadata::SqlMetadata, transparency_log::LogId};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) struct LogIntegrationState {
    lock: Arc<tokio::sync::Mutex<()>>,
    recovered: AtomicBool,
    reconciled: Mutex<HashSet<LogId>>,
}

impl Default for LogIntegrationState {
    fn default() -> Self {
        Self {
            lock: Arc::new(tokio::sync::Mutex::new(())),
            recovered: AtomicBool::new(false),
            reconciled: Mutex::new(HashSet::new()),
        }
    }
}

impl LogIntegrationState {
    fn is_reconciled(&self, log: &LogId) -> bool {
        self.reconciled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(log)
    }

    fn mark_reconciled(&self, log: LogId) {
        self.reconciled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(log);
    }
}

/// Reconcile every configured hosted log and every SQL-sequenced log against
/// `.transparency-log/` before the server accepts traffic.
pub(super) async fn reconcile_configured_logs(storage: &Storage, config: &Config) -> Result<()> {
    let key = config.release_records.log_signing_key.as_deref();
    if key.is_none() && !storage.has_any_stored_log().await? {
        return Ok(());
    }
    let logs = candidate_logs(storage.sql_metadata(), config).await?;
    for log in logs {
        storage.reconcile_log(&log, key).await?;
    }
    Ok(())
}

/// Ensure `log` has been reconciled against `.transparency-log/` on this
/// instance before a publish sequences new entries into it.
pub(super) async fn ensure_reconciled(state: &AppState, log: &LogId) -> Result<()> {
    if state.inner.locks.logs.is_reconciled(log) {
        return Ok(());
    }
    let _integrating = state.inner.locks.logs.lock.lock().await;
    if state.inner.locks.logs.is_reconciled(log) {
        return Ok(());
    }
    let key = state.inner.config.release_records.log_signing_key.as_deref();
    if key.is_none() && !state.inner.storage.has_stored_log(log).await? {
        state.inner.locks.logs.mark_reconciled(log.clone());
        return Ok(());
    }
    state.inner.storage.reconcile_log(log, key).await?;
    state.inner.locks.logs.mark_reconciled(log.clone());
    Ok(())
}

/// Integrate `log` in the background, after a commit that sequenced entries
/// into it.
pub(super) fn integrate_in_background(state: &AppState, log: LogId) {
    let Some(key) = state.inner.config.release_records.log_signing_key.clone() else {
        return;
    };
    integrate_sequenced_logs(state);
    let state = state.clone();
    tokio::spawn(async move {
        let _integrating = state.inner.locks.logs.lock.lock().await;
        match state.inner.storage.integrate_log(&log, &key).await {
            Ok(_) => state.inner.locks.logs.mark_reconciled(log),
            Err(error) => {
                tracing::warn!(%error, %log, "integrating a transparency log failed");
                // The next trigger scans every log again, this one included.
                state.inner.locks.logs.recovered.store(false, Ordering::Release);
            }
        }
    });
}

/// Integrate, in the background, every configured and sequenced log once per
/// server, as after an unclean shutdown or restore. Outside a Tokio runtime it
/// does nothing and leaves the scan to the next call.
pub(super) fn integrate_sequenced_logs(state: &AppState) {
    let Some(key) = state.inner.config.release_records.log_signing_key.clone() else {
        return;
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    if state.inner.locks.logs.recovered.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut retry = RetryRecovery(Some(state.clone()));
    let state = state.clone();
    runtime.spawn(async move {
        let _integrating = state.inner.locks.logs.lock.lock().await;
        if integrate_every_log(&state, &key).await {
            retry.disarm();
        }
    });
}

struct RetryRecovery(Option<AppState>);

impl RetryRecovery {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for RetryRecovery {
    fn drop(&mut self) {
        if let Some(state) = &self.0 {
            state.inner.locks.logs.recovered.store(false, Ordering::Release);
        }
    }
}

async fn integrate_every_log(state: &AppState, key: &NoteSigner) -> bool {
    let database = state.inner.storage.sql_metadata();
    let logs = match candidate_logs(database, &state.inner.config).await {
        Ok(logs) => logs,
        Err(error) => {
            tracing::warn!(%error, "listing the transparency logs to integrate failed");
            return false;
        }
    };
    let mut integrated = true;
    for log in logs {
        match state.inner.storage.integrate_log(&log, key).await {
            Ok(_) => state.inner.locks.logs.mark_reconciled(log),
            Err(error) => {
                tracing::warn!(%error, %log, "integrating a transparency log failed");
                integrated = false;
            }
        }
    }
    integrated
}

async fn candidate_logs(database: &Arc<SqlMetadata>, config: &Config) -> Result<Vec<LogId>> {
    let mut logs: Vec<LogId> = config.routing.hosted
        .iter()
        .filter_map(|(name, hosted)| {
            let ecosystem = config.routing.registries.ecosystem(name)?;
            Some(LogId { catalog: hosted.org.clone(), ecosystem })
        })
        .collect();
    logs.push(LogId { catalog: String::new(), ecosystem: Ecosystem::Npm });
    for (log, _) in database.sequenced_logs().await? {
        logs.push(log);
    }
    logs.sort_by(|left, right| {
        (left.catalog.as_str(), left.ecosystem.as_str()).cmp(&(
            right.catalog.as_str(),
            right.ecosystem.as_str(),
        ))
    });
    logs.dedup();
    Ok(logs)
}
