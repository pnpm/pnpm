//! Refetching a fresh cached packument that does not list a requested
//! version.

use super::{
    AdmittedPackument,
    AppState,
    CanonicalPackageName,
    DocumentSource,
    Duration,
    Identity,
    PackumentFlightKind,
    PackumentRead,
    RegistryError,
    RegistrySource,
    Upstream,
    authorized_upstream,
    coalesce_upstream_packument_keyed,
    fetch_and_cache_upstream_packument,
    fetch_coalesced_upstream_packument,
    read_unpurged_upstream_document,
    upstream_cache_namespace,
};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::Instant,
};

#[cfg(test)]
mod tests;

/// Minimum age of cached metadata before a missing version triggers a refetch.
pub(in crate::server) const REFETCH_INTERVAL: Duration = Duration::from_secs(60);

const CAPACITY: usize = 16 * 1024;

#[derive(Default)]
pub(in crate::server) struct PackumentRefetches {
    forced_attempts: Mutex<HashMap<(String, CanonicalPackageName), Instant>>,
}

impl PackumentRefetches {
    fn claim_forced(&self, namespace: String, name: &CanonicalPackageName) -> bool {
        let mut attempts = self.forced_attempts.lock().expect("packument refetch lock poisoned");
        let key = (namespace, name.clone());
        if attempts
            .get(&key)
            .is_some_and(|attempted| attempted.elapsed() < REFETCH_INTERVAL)
        {
            return false;
        }
        insert_attempt(&mut attempts, key);
        true
    }
}

fn insert_attempt(
    attempts: &mut HashMap<(String, CanonicalPackageName), Instant>,
    key: (String, CanonicalPackageName),
) {
    if attempts.len() >= CAPACITY {
        attempts.retain(|_, attempted| attempted.elapsed() < REFETCH_INTERVAL);
    }
    if attempts.len() >= CAPACITY {
        let evicted: Vec<_> = attempts
            .keys()
            .take(CAPACITY / 4)
            .cloned()
            .collect();
        for evicted in evicted {
            attempts.remove(&evicted);
        }
    }
    attempts.insert(key, Instant::now());
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

/// Read a sufficiently recent packument, revalidating older metadata upstream.
pub(in crate::server) async fn reload_upstream_packument_lacking_version(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if !upstream.caches() {
        return Ok(None);
    }
    if let Some(recent) =
        read_unpurged_upstream_document(state, namespace, name, REFETCH_INTERVAL).await?
    {
        return Ok(Some(recent));
    }
    let document = fetch_coalesced_upstream_packument(
        state,
        namespace,
        upstream,
        name,
        PackumentRead::MissingVersion,
    )
    .await?;
    Ok(document.map(|document| document.bytes.to_vec()))
}

/// Refetch a package's packument after a signed tarball URL failed with
/// `401`/`403`/`410`. Coordinated per package on [`PackumentFlightKind::ForcedRefresh`]
/// so concurrent expired downloads across versions share one post-rejection
/// refresh without joining an earlier [`PackumentFlightKind::Read`], while
/// throttling sequential forced refreshes via [`PackumentRefetches::claim_forced`].
pub(in crate::server) async fn reload_upstream_packument_for_expired_tarball(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    upstream.ensure_may_fetch()?;
    let key = (namespace.to_string(), name.clone(), PackumentFlightKind::ForcedRefresh);
    let document = coalesce_upstream_packument_keyed(state, key, || async {
        let admissions = &state.inner.packuments.admissions;
        let upstream_admission = admissions.acquire(DocumentSource::Upstream(namespace)).await;
        if !state.inner.proxy.packument_refetches.claim_forced(namespace.to_owned(), name) {
            drop(upstream_admission);
            if !upstream.caches() {
                return Ok(None);
            }
            let admission = admissions.acquire(DocumentSource::Stored).await;
            let cached =
                read_unpurged_upstream_document(state, namespace, name, REFETCH_INTERVAL).await?;
            return Ok(cached.map(|bytes| AdmittedPackument::new(bytes, admission)));
        }
        let fetched =
            fetch_and_cache_upstream_packument(state, namespace, upstream, name, None).await?;
        Ok(fetched.map(|bytes| AdmittedPackument::new(bytes, upstream_admission)))
    })
    .await?;
    Ok(document.map(|document| document.bytes.to_vec()))
}
