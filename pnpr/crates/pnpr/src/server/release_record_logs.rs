//! Integrating and reconciling the transparency logs of hosted release records
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#transparency-log)).
//!
//! A record's log entry is sequenced when its version is accepted. A commit
//! that sequenced entries integrates their log in the background, and each
//! server once integrates every log, which picks up whatever an interrupted
//! integration left: when its router is built, or, for a router built
//! outside a Tokio runtime, at the first integration it starts or the
//! first read of a log. Startup and publish commits reconcile each log against
//! `.transparency-log/` before new entries are sequenced, re-importing
//! checkpoint-signed entries from stored bundles after a metadata rollback and
//! refusing forked logs.

use super::AppState;
use pnpm_transparency_log::NoteSigner;
use pnpr_config::Config;
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use pnpr_storage::{
    Storage,
    metadata::SqlMetadata,
    transparency_log::{LogId, StoredCheckpoint},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(super) struct LogIntegrationState {
    locks: Mutex<HashMap<LogId, Arc<tokio::sync::Mutex<()>>>>,
    recovered: AtomicBool,
    reconciled: Mutex<HashMap<LogId, Option<StoredCheckpoint>>>,
}

impl LogIntegrationState {
    fn lock_for(&self, log: &LogId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(
            locks
                .entry(log.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn matching_checkpoint(
        &self,
        log: &LogId,
        current: Option<&StoredCheckpoint>,
    ) -> Option<Option<StoredCheckpoint>> {
        self.reconciled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(log)
            .filter(|cached| cached.as_ref() == current)
            .cloned()
    }

    fn mark_reconciled(&self, log: LogId, checkpoint: Option<StoredCheckpoint>) {
        self.reconciled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(log, checkpoint);
    }

    fn unmark_reconciled(&self, log: &LogId) {
        self.reconciled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(log);
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
    let current = state.inner.storage.log_checkpoint(log).await?;
    if state.inner.locks.logs.matching_checkpoint(log, current.as_ref()).is_some() {
        return Ok(());
    }
    let lock = state.inner.locks.logs.lock_for(log);
    let _guard = lock.lock().await;
    let key = state.inner.config.release_records.log_signing_key.as_deref();
    ensure_reconciled_locked(state, log, key).await.map(|_| ())
}

async fn ensure_reconciled_locked(
    state: &AppState,
    log: &LogId,
    key: Option<&NoteSigner>,
) -> Result<Option<StoredCheckpoint>> {
    let current = state.inner.storage.log_checkpoint(log).await?;
    if let Some(verified) = state.inner.locks.logs.matching_checkpoint(log, current.as_ref()) {
        return Ok(verified);
    }
    state.inner.locks.logs.unmark_reconciled(log);
    if key.is_none() && current.is_none() && !state.inner.storage.has_stored_log(log).await? {
        state.inner.locks.logs.mark_reconciled(log.clone(), None);
        return Ok(None);
    }
    let verified = state.inner.storage.reconcile_log(log, key).await?;
    state.inner.locks.logs.mark_reconciled(log.clone(), verified.clone());
    Ok(verified)
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
        if !integrate_one_log(&state, &log, &key).await {
            // The next trigger scans every log again, this one included.
            state.inner.locks.logs.recovered.store(false, Ordering::Release);
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
        if !integrate_one_log(state, &log, key).await {
            integrated = false;
        }
    }
    integrated
}

async fn integrate_one_log(state: &AppState, log: &LogId, key: &NoteSigner) -> bool {
    let lock = state.inner.locks.logs.lock_for(log);
    let _guard = lock.lock().await;
    let outcome = async {
        let verified = ensure_reconciled_locked(state, log, Some(key)).await?;
        let (_, stored) = state.inner.storage.integrate_reconciled_log(log, key, verified).await?;
        state.inner.locks.logs.mark_reconciled(log.clone(), stored);
        Ok::<_, pnpr_error::RegistryError>(())
    }
    .await;
    if let Err(error) = outcome {
        state.inner.locks.logs.unmark_reconciled(log);
        tracing::warn!(%error, %log, "integrating a transparency log failed");
        return false;
    }
    true
}

async fn candidate_logs(database: &Arc<SqlMetadata>, config: &Config) -> Result<Vec<LogId>> {
    let mut logs: Vec<LogId> = config.routing.hosted
        .iter()
        .filter_map(|(name, hosted)| {
            let ecosystem = config.routing.registries.ecosystem(name)?;
            Some(LogId { namespace: hosted.namespace.clone(), ecosystem })
        })
        .collect();
    logs.push(LogId { namespace: String::new(), ecosystem: Ecosystem::Npm });
    for (log, _) in database.sequenced_logs().await? {
        logs.push(log);
    }
    logs.sort_by(|left, right| {
        (left.namespace.as_str(), left.ecosystem.as_str()).cmp(&(
            right.namespace.as_str(),
            right.ecosystem.as_str(),
        ))
    });
    logs.dedup();
    Ok(logs)
}
