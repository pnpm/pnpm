use std::{future::Future, time::Duration};

use super::{
    AppState, CacheValidators, CanonicalPackageName, PackumentFetch, PurgeRemoval, RegistryError,
    Upstream, WriteVerdict, invalidate_upstream_package, lock_upstream_package, timed,
};
use crate::server::upstream_integrity::{RefreshOutcome, refresh_cached_packument, schedule};

pub(in crate::server) enum PurgeOutcome {
    Purged,
    SupersededByStored(Vec<u8>),
    MissingSupersedingStored,
}

enum CachedPackumentStep {
    Done(Option<Vec<u8>>),
    RetryAfterEviction,
}

pub(in crate::server) async fn cache_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    fetched: PackumentFetch,
    fetch_seq: u64,
) -> Result<Option<Vec<u8>>, RegistryError> {
    match apply_upstream_packument(state, namespace, upstream, name, fetched, fetch_seq).await? {
        CachedPackumentStep::Done(result) => Ok(result),
        CachedPackumentStep::RetryAfterEviction => {
            Box::pin(refetch_evicted_packument(state, namespace, upstream, name)).await
        }
    }
}

async fn apply_upstream_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    fetched: PackumentFetch,
    fetch_seq: u64,
) -> Result<CachedPackumentStep, RegistryError> {
    match fetched {
        PackumentFetch::Modified(fetched) => {
            apply_modified_packument(state, namespace, upstream, name, fetched.bytes, fetch_seq)
                .await
        }
        PackumentFetch::NotFound => {
            apply_not_found_packument(state, namespace, upstream, name, fetch_seq).await
        }
        PackumentFetch::NotModified => {
            let cached = read_unpurged_upstream_document_any(state, namespace, name).await?;
            Ok(CachedPackumentStep::Done(cached))
        }
    }
}

async fn apply_modified_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    bytes: Vec<u8>,
    fetch_seq: u64,
) -> Result<CachedPackumentStep, RegistryError> {
    if !upstream.caches() {
        return Ok(CachedPackumentStep::Done(Some(bytes)));
    }
    match refresh_cached_packument(state, namespace, name, bytes, fetch_seq).await? {
        RefreshOutcome::Stored(refreshed) => {
            if refreshed.needs_backfill {
                schedule(state, namespace, upstream, name);
            }
            Ok(CachedPackumentStep::Done(Some(refreshed.bytes)))
        }
        RefreshOutcome::SupersededByPurge => Ok(CachedPackumentStep::Done(None)),
        RefreshOutcome::MissingSupersedingStored => Ok(CachedPackumentStep::RetryAfterEviction),
    }
}

async fn apply_not_found_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    fetch_seq: u64,
) -> Result<CachedPackumentStep, RegistryError> {
    if !upstream.caches() {
        return Ok(CachedPackumentStep::Done(None));
    }
    match purge_cached_package(state, namespace, name, fetch_seq).await? {
        PurgeOutcome::Purged => Ok(CachedPackumentStep::Done(None)),
        PurgeOutcome::SupersededByStored(bytes) => Ok(CachedPackumentStep::Done(Some(bytes))),
        PurgeOutcome::MissingSupersedingStored => Ok(CachedPackumentStep::RetryAfterEviction),
    }
}

async fn refetch_evicted_packument(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let retry_seq = state.inner.proxy.packument_writes.begin_fetch();
    let fetched = timed(
        "packument:upstream_fetch",
        name.as_str(),
        upstream.fetch_packument(name, &CacheValidators::default()),
    )
    .await?;
    let fallback = match &fetched {
        PackumentFetch::Modified(doc) => Some(doc.bytes.clone()),
        PackumentFetch::NotFound | PackumentFetch::NotModified => None,
    };
    match apply_upstream_packument(state, namespace, upstream, name, fetched, retry_seq).await? {
        CachedPackumentStep::Done(result) => Ok(result),
        CachedPackumentStep::RetryAfterEviction => Ok(fallback),
    }
}

pub(super) async fn read_unpurged_upstream_document(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    ttl: Duration,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if suppress_if_purged(state, namespace, name).await? {
        return Ok(None);
    }
    state.inner.storage.read_upstream_document(namespace, name, ttl).await
}

pub(super) async fn read_unpurged_upstream_document_any(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) -> Result<Option<Vec<u8>>, RegistryError> {
    if suppress_if_purged(state, namespace, name).await? {
        return Ok(None);
    }
    state.inner.storage.read_upstream_document_any(namespace, name).await
}

async fn suppress_if_purged(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) -> Result<bool, RegistryError> {
    if !state.inner.proxy.packument_writes.is_purged(namespace, name)
        && !state.inner.storage.is_upstream_package_purged(namespace, name).await?
    {
        return Ok(false);
    }
    Box::pin(suppress_if_purged_slow(state, namespace, name)).await
}

async fn suppress_if_purged_slow(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
) -> Result<bool, RegistryError> {
    let _guard = lock_upstream_package(state, namespace, name).await;
    if state.inner.storage.is_upstream_package_purged(namespace, name).await? {
        if state.inner.storage.remove_upstream_package(namespace, name).await.is_ok() {
            state.inner.proxy.packument_writes.resolve_purge(namespace, name);
        }
        return Ok(true);
    }
    Ok(state.inner.proxy.packument_writes.is_purged(namespace, name))
}

pub(super) async fn purge_cached_package(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    fetch_seq: u64,
) -> Result<PurgeOutcome, RegistryError> {
    let remove_state = state.clone();
    let ns = namespace.to_string();
    let pkg = name.clone();
    purge_cached_package_with(state, namespace, name, fetch_seq, move || async move {
        remove_state.inner.storage.remove_upstream_package(&ns, &pkg).await
    })
    .await
}

pub(in crate::server) async fn purge_cached_package_with<Remove, Fut>(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    fetch_seq: u64,
    remove: Remove,
) -> Result<PurgeOutcome, RegistryError>
where
    Remove: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<bool, RegistryError>> + Send + 'static,
{
    let state = state.clone();
    let namespace = namespace.to_string();
    let name = name.clone();
    tokio::spawn(
        async move { run_locked_purge(&state, &namespace, &name, fetch_seq, remove).await },
    )
    .await
    .map_err(RegistryError::JoinError)?
}

async fn run_locked_purge<Remove, Fut>(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    fetch_seq: u64,
    remove: Remove,
) -> Result<PurgeOutcome, RegistryError>
where
    Remove: FnOnce() -> Fut,
    Fut: Future<Output = Result<bool, RegistryError>>,
{
    let _guard = lock_upstream_package(state, namespace, name).await;
    match state.inner.proxy.packument_writes.check(namespace, name, fetch_seq) {
        WriteVerdict::SupersededByPurge => return Ok(PurgeOutcome::Purged),
        WriteVerdict::SupersededByStored => {
            let cached = state.inner.storage.read_upstream_document_any(namespace, name).await?;
            return Ok(match cached {
                Some(bytes) => PurgeOutcome::SupersededByStored(bytes),
                None => PurgeOutcome::MissingSupersedingStored,
            });
        }
        WriteVerdict::Proceed => {}
    }
    invalidate_upstream_package(state, namespace, name);
    let removal = complete_upstream_purge(state, namespace, name, remove).await;
    state.inner.proxy.packument_writes.commit_purged(namespace, name, fetch_seq, removal);
    Ok(PurgeOutcome::Purged)
}

async fn complete_upstream_purge<Remove, Fut>(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    remove: Remove,
) -> PurgeRemoval
where
    Remove: FnOnce() -> Fut,
    Fut: Future<Output = Result<bool, RegistryError>>,
{
    match remove().await {
        Ok(_) => match state.inner.storage.clear_upstream_package_purged(namespace, name).await {
            Ok(()) => PurgeRemoval::Complete,
            Err(err) => {
                tracing::warn!(
                    ?err,
                    package = %name.as_str(),
                    "failed to clear durable purge marker after an upstream 404",
                );
                PurgeRemoval::Pending
            }
        },
        Err(err) => {
            if let Err(marker_err) =
                state.inner.storage.mark_upstream_package_purged(namespace, name).await
            {
                tracing::warn!(
                    ?err,
                    ?marker_err,
                    package = %name.as_str(),
                    "failed to purge cached entry or persist purge marker after an upstream 404",
                );
            } else {
                tracing::warn!(
                    ?err,
                    package = %name.as_str(),
                    "failed to purge cached entry after an upstream 404; persisted purge marker",
                );
            }
            PurgeRemoval::Pending
        }
    }
}
