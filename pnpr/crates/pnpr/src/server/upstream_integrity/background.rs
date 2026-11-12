use super::{AppState, CanonicalPackageName, Upstream, complete_missing_tarball_integrities};
use pnpr_storage::{BoundedCache, DocumentStamp};
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::{Semaphore, watch};

const MAX_ACTIVE_PACKUMENTS: usize = 8;
const MAX_OUTSTANDING_PACKUMENTS: usize = 256;

type JobKey = (String, String);

pub(in crate::server) struct BackgroundIntegrity {
    jobs: Mutex<HashMap<JobKey, PendingJob>>,
    attempted: Mutex<BoundedCache<JobKey, DocumentStamp>>,
    // These semaphores are never closed, so acquiring permits cannot fail.
    slots: Semaphore,
    pub(super) downloads: Semaphore,
}

struct PendingJob {
    changed: bool,
    completion: watch::Sender<()>,
}

impl Default for BackgroundIntegrity {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
            slots: Semaphore::new(MAX_ACTIVE_PACKUMENTS),
            attempted: Mutex::default(),
            downloads: Semaphore::new(super::MAX_CONCURRENT_PIN_DOWNLOADS),
        }
    }
}

struct Job {
    state: AppState,
    key: JobKey,
    completed: bool,
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.completed {
            self.state.inner.proxy.integrity_backfill.jobs
                .lock()
                .unwrap()
                .remove(&self.key);
        }
    }
}

#[derive(Clone, Copy)]
enum Trigger {
    Refresh,
    Resume(DocumentStamp),
}

pub(in crate::server) fn schedule(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) {
    if let Err(err) = start(state, namespace, upstream, name, Trigger::Refresh) {
        tracing::warn!(%err, package = %name.as_str(), "integrity backfill queue is full");
    }
}

fn start(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    trigger: Trigger,
) -> Result<(), super::RegistryError> {
    let key = (namespace.to_string(), name.as_str().to_string());
    {
        let mut jobs = state.inner.proxy.integrity_backfill.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&key) {
            job.changed |= matches!(trigger, Trigger::Refresh);
            return Ok(());
        }
        if let Trigger::Resume(stamp) = trigger
            && state.inner.proxy.integrity_backfill.attempted
                .lock()
                .unwrap()
                .get(&key)
                == Some(&stamp)
        {
            return Ok(());
        }
        if jobs.len() >= MAX_OUTSTANDING_PACKUMENTS {
            return Err(super::RegistryError::TooManyRequests {
                reason: format!("integrity backfill queue is full for {}", name.as_str()),
            });
        }
        jobs.insert(key.clone(), PendingJob { changed: false, completion: watch::channel(()).0 });
    }
    let job = Job { state: state.clone(), key, completed: false };
    let upstream = upstream.clone();
    let name = name.clone();
    tokio::spawn(job.run(upstream, name));
    Ok(())
}

pub(in crate::server) async fn wait_for_pin(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<(), super::RegistryError> {
    let key = (namespace.to_string(), name.as_str().to_string());
    let Some(stamp) = state.inner.storage.upstream_document_stamp(namespace, name).await? else {
        return Ok(());
    };
    let has_pending = state.inner.proxy.integrity_backfill.jobs
        .lock()
        .unwrap()
        .contains_key(&key);
    if !has_pending
        && state.inner.proxy.integrity_backfill.attempted
            .lock()
            .unwrap()
            .get(&key)
            == Some(&stamp)
    {
        return Ok(());
    }
    if !eligible_for_pin(state, namespace, name, filename).await? {
        return Ok(());
    }
    start(state, namespace, upstream, name, Trigger::Resume(stamp))?;
    if tokio::time::timeout(upstream.timeout(), wait(state, namespace, name))
        .await
        .is_err()
    {
        tracing::warn!(package = %name.as_str(), "timed out waiting for integrity backfill");
    }
    Ok(())
}

async fn eligible_for_pin(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<bool, super::RegistryError> {
    let Some(bytes) = state.inner.storage.read_upstream_document_any(namespace, name).await? else {
        return Ok(false);
    };
    let package = name.clone();
    let filename = filename.to_string();
    let osv_index = state.inner.osv_index.clone();
    state.inner.packuments.workers.run(move || {
        let doc = serde_json::from_slice(&bytes)?;
        Ok::<_, super::RegistryError>(
            super::missing_integrity_tarballs(&doc, &package, osv_index.as_ref())
                .into_iter()
                .take(super::MAX_PINNED_VERSIONS_PER_PACKUMENT)
                .any(|candidate| candidate.filename == filename),
        )
    })
    .await?
}

pub(in crate::server) async fn wait(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) {
    let receiver = state.inner.proxy.integrity_backfill.jobs
        .lock()
        .unwrap()
        .get(&(namespace.to_string(), name.as_str().to_string()))
        .map(|job| job.completion.subscribe());
    if let Some(mut receiver) = receiver {
        while receiver.changed().await.is_ok() {}
    }
}

impl Job {
    async fn run(mut self, upstream: Upstream, name: CanonicalPackageName) {
        loop {
            let permit = self.state.inner.proxy.integrity_backfill.slots.acquire().await.unwrap();
            let result = self.complete(&upstream, &name).await;
            drop(permit);
            if let Err(err) = &result {
                tracing::warn!(%err, package = %name.as_str(), "integrity backfill failed");
            }
            let mut jobs = self.state.inner.proxy.integrity_backfill.jobs.lock().unwrap();
            if std::mem::take(&mut jobs.get_mut(&self.key).unwrap().changed) {
                continue;
            }
            if let Ok(Some(stamp)) = result {
                self.state.inner.proxy.integrity_backfill.attempted
                    .lock()
                    .unwrap()
                    .insert(self.key.clone(), stamp, 1, 16 * 1024);
            }
            jobs.remove(&self.key);
            self.completed = true;
            break;
        }
    }

    async fn complete(
        &self,
        upstream: &Upstream,
        name: &CanonicalPackageName,
    ) -> Result<Option<DocumentStamp>, super::RegistryError> {
        let Some(bytes) =
            self.state.inner.storage.read_upstream_document_any(&self.key.0, name).await?
        else {
            return Ok(None);
        };
        complete_missing_tarball_integrities(&self.state, &self.key.0, upstream, name, bytes)
            .await?;
        self.state.inner.storage.upstream_document_stamp(&self.key.0, name).await
    }
}

#[cfg(test)]
mod tests;
