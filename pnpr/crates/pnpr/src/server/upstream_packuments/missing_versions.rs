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
    RegistryError,
    RegistrySource,
    Upstream,
    authorized_upstream,
    coalesce_upstream_packument,
    coalesce_upstream_packument_keyed,
    fetch_and_cache_upstream_packument,
    read_unpurged_upstream_document,
    read_unpurged_upstream_document_any,
    upstream_cache_namespace,
};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::Instant,
};

#[cfg(test)]
mod tests;

/// How long after a packument's fetch, a failed fetch, or a refetch attempt,
/// a request for a version it does not list waits before refetching it. A
/// version published upstream after the fetch is then served before the
/// packument TTL expires, and requests for versions that do not exist reach
/// the upstream at most once per package per interval.
pub(in crate::server) const REFETCH_INTERVAL: Duration = Duration::from_secs(60);

/// Packages tracked before attempts are dropped: those older than
/// [`REFETCH_INTERVAL`], and a quarter of the rest if that frees none.
const CAPACITY: usize = 16 * 1024;

/// When each package's packument was last refetched for a version it did
/// not list, whether or not the refetch succeeded.
#[derive(Default)]
pub(in crate::server) struct PackumentRefetches {
    attempts: Mutex<HashMap<(String, CanonicalPackageName), Instant>>,
    forced_attempts: Mutex<HashMap<(String, CanonicalPackageName), Instant>>,
}

impl PackumentRefetches {
    /// Record an attempt for the package. `false` when one was recorded
    /// within [`REFETCH_INTERVAL`].
    fn claim(&self, namespace: &str, name: &CanonicalPackageName) -> bool {
        claim_attempt(&self.attempts, namespace, name)
    }

    /// Record a forced-refresh attempt after an expired signed tarball URL.
    /// Tracked separately from [`Self::record_failure`] so an earlier ordinary
    /// read failure does not delay rotating an expired signature.
    fn claim_forced(&self, namespace: &str, name: &CanonicalPackageName) -> bool {
        claim_attempt(&self.forced_attempts, namespace, name)
    }

    /// Record a failed upstream fetch of the package's packument, so a
    /// missing-version refetch does not ask the upstream again within
    /// [`REFETCH_INTERVAL`].
    pub(in crate::server) fn record_failure(&self, namespace: &str, name: &CanonicalPackageName) {
        let mut attempts = self.attempts.lock().expect("packument refetch lock poisoned");
        insert_attempt(&mut attempts, (namespace.to_string(), name.clone()));
    }
}

fn claim_attempt(
    map: &Mutex<HashMap<(String, CanonicalPackageName), Instant>>,
    namespace: &str,
    name: &CanonicalPackageName,
) -> bool {
    let mut attempts = map.lock().expect("packument refetch lock poisoned");
    let key = (namespace.to_string(), name.clone());
    if attempts
        .get(&key)
        .is_some_and(|attempted| attempted.elapsed() < REFETCH_INTERVAL)
    {
        return false;
    }
    insert_attempt(&mut attempts, key);
    true
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

/// The packument to read again after its fresh cached copy did not list a
/// requested version:
///
/// - the cached copy if it was fetched within [`REFETCH_INTERVAL`], as
///   another request may have refetched it since;
/// - the result of a refetch already in flight for the package;
/// - a refetch, unless a refetch or a failed fetch was attempted within
///   [`REFETCH_INTERVAL`], which leaves the cached copy.
///
/// `None` when the upstream is not cached.
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
    // The claim is taken by the flight's leader, so a request arriving
    // during a refetch waits for it rather than finding the claim taken.
    let document = coalesce_upstream_packument(state, namespace, name, || async {
        let admissions = &state.inner.packuments.admissions;
        if !state.inner.proxy.packument_refetches.claim(namespace, name) {
            let admission = admissions.acquire(DocumentSource::Stored).await;
            let cached = read_unpurged_upstream_document_any(state, namespace, name).await?;
            return Ok(cached.map(|bytes| AdmittedPackument::new(bytes, admission)));
        }
        let admission = admissions.acquire(DocumentSource::Upstream(namespace)).await;
        let fetched = fetch_and_cache_upstream_packument(
            state,
            namespace,
            upstream,
            name,
            Some(REFETCH_INTERVAL),
        )
        .await?;
        Ok(fetched.map(|bytes| AdmittedPackument::new(bytes, admission)))
    })
    .await?;
    Ok(document.map(|document| document.bytes.to_vec()))
}

/// Refetch a package's packument after a signed tarball URL failed with
/// `401`/`403`/`410`. Coordinated per package on [`PackumentFlightKind::ForcedRefresh`]
/// so concurrent expired downloads across versions share one post-rejection
/// refresh without joining an earlier [`PackumentFlightKind::Read`], while
/// throttling sequential forced refreshes via [`PackumentRefetches::claim`].
pub(in crate::server) async fn reload_upstream_packument_for_expired_tarball(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let key = (namespace.to_string(), name.clone(), PackumentFlightKind::ForcedRefresh);
    let document = coalesce_upstream_packument_keyed(state, key, || async {
        let admissions = &state.inner.packuments.admissions;
        let upstream_admission = admissions.acquire(DocumentSource::Upstream(namespace)).await;
        if !state.inner.proxy.packument_refetches.claim_forced(namespace, name) {
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
