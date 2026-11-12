use super::{
    AppState, RegistryError, Response, UpstreamTarball, cached_upstream_tarball, streaming,
    tarball_response,
};
use crate::server::{
    upstream_integrity::cached::hash_cached_file, upstream_packuments::lock_upstream_package,
};
use tokio::io::AsyncSeekExt;

pub(super) async fn bound_cached_tarball(
    state: &AppState,
    tarball: &UpstreamTarball<'_>,
) -> Result<Option<Response>, RegistryError> {
    let UpstreamTarball { namespace, name, filename, .. } = *tarball;
    if tarball.manifest_generation.is_none() {
        return Ok(cached_upstream_tarball(state, namespace, name, filename).await);
    }
    let _guard = lock_upstream_package(state, namespace, name).await;
    let Some((mut file, len)) =
        state.inner.storage.open_upstream_blob(namespace, name, filename).await?
    else {
        return Ok(None);
    };
    let (_, checker) = hash_cached_file(&mut file, Some(tarball.integrity)).await?;
    if checker.unwrap().result().is_err() {
        drop(file);
        state.inner.storage.remove_upstream_blob(namespace, name, filename).await?;
        return Ok(None);
    }
    file.rewind().await?;
    Ok(Some(tarball_response(streaming::stream_file(file), Some(len))))
}
