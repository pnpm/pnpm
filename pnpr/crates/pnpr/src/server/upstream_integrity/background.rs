use super::{AppState, CanonicalPackageName, Upstream, complete_missing_tarball_integrities};
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::{Semaphore, watch};

const MAX_ACTIVE_PACKUMENTS: usize = 8;

type JobKey = (String, String);

pub(in crate::server) struct BackgroundIntegrity {
    jobs: Mutex<HashMap<JobKey, PendingJob>>,
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

pub(in crate::server) fn schedule(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) {
    let key = (namespace.to_string(), name.as_str().to_string());
    {
        let mut jobs = state.inner.proxy.integrity_backfill.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&key) {
            job.changed = true;
            return;
        }
        jobs.insert(key.clone(), PendingJob { changed: false, completion: watch::channel(()).0 });
    }
    let job = Job { state: state.clone(), key, completed: false };
    let upstream = upstream.clone();
    let name = name.clone();
    tokio::spawn(job.run(upstream, name));
}

pub(in crate::server) async fn wait_for_pin(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<(), super::RegistryError> {
    let Some(bytes) = state.inner.storage.read_upstream_document_any(namespace, name).await? else {
        return Ok(());
    };
    let package = name.clone();
    let filename = filename.to_string();
    let eligible = state.inner.packuments.workers.run(move || {
        let doc = serde_json::from_slice(&bytes)?;
        Ok::<_, super::RegistryError>(
            super::missing_integrity_tarballs(&doc, &package, None)
                .into_iter()
                .take(super::MAX_PINNED_VERSIONS_PER_PACKUMENT)
                .any(|candidate| candidate.filename == filename),
        )
    })
    .await??;
    // Only refresh starts work; failed pins are retried at the next refresh.
    if eligible
        && tokio::time::timeout(upstream.timeout(), wait(state, namespace, name))
            .await
            .is_err()
    {
        tracing::warn!(package = %name.as_str(), "timed out waiting for integrity backfill");
    }
    Ok(())
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
            if let Err(err) = result {
                tracing::warn!(%err, package = %name.as_str(), "integrity backfill failed");
            }
            let mut jobs = self.state.inner.proxy.integrity_backfill.jobs.lock().unwrap();
            if std::mem::take(&mut jobs.get_mut(&self.key).unwrap().changed) {
                continue;
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
    ) -> Result<(), super::RegistryError> {
        let Some(bytes) =
            self.state.inner.storage.read_upstream_document_any(&self.key.0, name).await?
        else {
            return Ok(());
        };
        complete_missing_tarball_integrities(&self.state, &self.key.0, upstream, name, bytes)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
