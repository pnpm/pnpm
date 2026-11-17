//! Refetching a fresh cached packument that does not list a requested
//! version.

use super::{
    AppState, CanonicalPackageName, Duration, Identity, RegistryError, RegistrySource, Upstream,
    authorized_upstream, fetch_coalesced_upstream_packument, upstream_cache_namespace,
};
use std::{collections::HashMap, sync::Mutex, time::Instant};

#[cfg(test)]
mod tests;

/// How long after a packument's fetch, or after an attempt to refetch it, a
/// request for a version it does not list waits before refetching it. A
/// version published upstream after the fetch is then served before the
/// packument TTL expires, and requests for versions that do not exist reach
/// the upstream at most once per package per interval.
const REFETCH_INTERVAL: Duration = Duration::from_secs(60);

/// Packages tracked before the attempts older than [`REFETCH_INTERVAL`] are
/// dropped.
const CAPACITY: usize = 16 * 1024;

/// When each package's packument was last refetched for a version it did
/// not list, whether or not the refetch succeeded.
#[derive(Default)]
pub(in crate::server) struct PackumentRefetches {
    attempts: Mutex<HashMap<(String, CanonicalPackageName), Instant>>,
}

impl PackumentRefetches {
    /// Record an attempt for the package. `false` when one was recorded
    /// within [`REFETCH_INTERVAL`], or when [`CAPACITY`] packages all have
    /// one that recent.
    fn claim(&self, namespace: &str, name: &CanonicalPackageName) -> bool {
        let mut attempts = self.attempts.lock().expect("packument refetch lock poisoned");
        let key = (namespace.to_string(), name.clone());
        if attempts
            .get(&key)
            .is_some_and(|attempted| attempted.elapsed() < REFETCH_INTERVAL)
        {
            return false;
        }
        if attempts.len() >= CAPACITY {
            attempts.retain(|_, attempted| attempted.elapsed() < REFETCH_INTERVAL);
            if attempts.len() >= CAPACITY {
                return false;
            }
        }
        attempts.insert(key, Instant::now());
        true
    }
}

/// [`reload_upstream_packument_lacking_version`] for the upstream `source`
/// routes the package to. `None` for any other source.
pub(in crate::server) async fn reload_source_packument_lacking_version(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let RegistrySource::Upstream(id) = source else { return Ok(None) };
    let namespace = upstream_cache_namespace(state, id);
    let upstream = authorized_upstream(state, identity, id)?;
    reload_upstream_packument_lacking_version(state, &namespace, &upstream, name).await
}

/// The packument to read again after its fresh cached copy did not list a
/// requested version: the cached copy if it was fetched within
/// [`REFETCH_INTERVAL`], as another request may have refetched it since,
/// else a refetch. `None` when nothing newer can be read:
///
/// - the upstream is not cached;
/// - the cached copy is past its TTL, so the caller's read already asked the
///   upstream;
/// - a refetch was attempted within [`REFETCH_INTERVAL`].
pub(in crate::server) async fn reload_upstream_packument_lacking_version(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if !upstream.caches() {
        return Ok(None);
    }
    let storage = &state.inner.storage;
    if let Some(recent) = storage.read_upstream_document(namespace, name, REFETCH_INTERVAL).await? {
        return Ok(Some(recent));
    }
    let ttl = upstream.packument_ttl().unwrap_or(state.inner.config.http.packument_ttl);
    if storage.fresh_upstream_document_stamp(namespace, name, ttl).await?.is_none()
        || !state.inner.proxy.packument_refetches.claim(namespace, name)
    {
        return Ok(None);
    }
    let document =
        fetch_coalesced_upstream_packument(state, namespace, upstream, name, REFETCH_INTERVAL)
            .await?;
    Ok(document.map(|document| document.bytes.to_vec()))
}
