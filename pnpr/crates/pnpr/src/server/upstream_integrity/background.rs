use super::{AppState, CanonicalPackageName, Upstream, complete_missing_tarball_integrities};
use std::{collections::HashSet, sync::Mutex};

const MAX_ACTIVE_PACKUMENTS: usize = 8;

#[derive(Default)]
pub(in crate::server) struct BackgroundIntegrity {
    active: Mutex<HashSet<(String, String)>>,
}

struct Job {
    state: AppState,
    key: (String, String),
}

impl Drop for Job {
    fn drop(&mut self) {
        self.state.inner.proxy.integrity_backfill.active
            .lock()
            .unwrap()
            .remove(&self.key);
    }
}

pub(in crate::server) fn schedule(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    bytes: Vec<u8>,
) {
    let key = (namespace.to_string(), name.as_str().to_string());
    let mut active = state.inner.proxy.integrity_backfill.active.lock().unwrap();
    if active.len() >= MAX_ACTIVE_PACKUMENTS || !active.insert(key.clone()) {
        return;
    }
    let job = Job { state: state.clone(), key };
    let upstream = upstream.clone();
    let name = name.clone();
    tokio::spawn(async move {
        if let Err(err) =
            complete_missing_tarball_integrities(&job.state, &job.key.0, &upstream, &name, bytes)
                .await
        {
            tracing::warn!(%err, package = %name.as_str(), "integrity backfill failed");
        }
    });
}
