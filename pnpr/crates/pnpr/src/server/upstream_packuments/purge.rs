use std::{future::Future, time::Duration};

use super::{
    AppState, CanonicalPackageName, RegistryError, WriteVerdict, invalidate_upstream_package,
    lock_upstream_package,
};

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
) -> Result<Option<Vec<u8>>, RegistryError> {
    purge_cached_package_with(state, namespace, name, fetch_seq, || {
        state.inner.storage.remove_upstream_package(namespace, name)
    })
    .await
}

pub(in crate::server) async fn purge_cached_package_with<Remove, Fut>(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    fetch_seq: u64,
    remove: Remove,
) -> Result<Option<Vec<u8>>, RegistryError>
where
    Remove: FnOnce() -> Fut,
    Fut: Future<Output = Result<bool, RegistryError>>,
{
    let _guard = lock_upstream_package(state, namespace, name).await;
    match state.inner.proxy.packument_writes.check(namespace, name, fetch_seq) {
        WriteVerdict::SupersededByPurge => return Ok(None),
        WriteVerdict::SupersededByStored => {
            return state.inner.storage.read_upstream_document_any(namespace, name).await;
        }
        WriteVerdict::Proceed => {}
    }
    invalidate_upstream_package(state, namespace, name);
    let removal_complete = complete_upstream_purge(state, namespace, name, remove).await;
    state.inner.proxy.packument_writes.commit_purged(namespace, name, fetch_seq, removal_complete);
    Ok(None)
}

async fn complete_upstream_purge<Remove, Fut>(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    remove: Remove,
) -> bool
where
    Remove: FnOnce() -> Fut,
    Fut: Future<Output = Result<bool, RegistryError>>,
{
    match remove().await {
        Ok(_) => match state.inner.storage.clear_upstream_package_purged(namespace, name).await {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(
                    ?err,
                    package = %name.as_str(),
                    "failed to clear durable purge marker after an upstream 404",
                );
                false
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
            false
        }
    }
}
