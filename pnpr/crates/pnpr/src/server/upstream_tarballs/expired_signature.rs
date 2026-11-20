use super::{
    AppState,
    FetchOutcome,
    RegistryError,
    TarballDist,
    Upstream,
    UpstreamTarball,
    UpstreamTarballRoute,
    parse_tarball_dist,
    timed,
};

pub(super) fn should_refresh_signed_tarball_url(
    route: &UpstreamTarballRoute,
    err: &RegistryError,
) -> bool {
    matches!(route, UpstreamTarballRoute::Url(url) if url.contains('?'))
        && matches!(err, RegistryError::UpstreamStatus { status: 401 | 403 | 410, .. })
}

pub(super) async fn retry_expired_signed_tarball(
    state: &AppState,
    upstream: &Upstream,
    tarball: UpstreamTarball<'_>,
    initial_err: RegistryError,
) -> Result<FetchOutcome<pnpm_network::ThrottledResponse>, RegistryError> {
    if !matches!(tarball.route, UpstreamTarballRoute::Url(_)) {
        return Err(initial_err);
    }
    let mut rotated = recent_rotated_tarball_dist(state, upstream, tarball).await?;
    if rotated.is_none()
        && let Some(packument) =
            crate::server::upstream_packuments::reload_upstream_packument_for_expired_tarball(
                state,
                tarball.namespace,
                upstream,
                tarball.name,
            )
            .await?
    {
        rotated = rotated_tarball_dist(state, packument, tarball).await?;
    }
    let Some(dist) = rotated else {
        return Err(initial_err);
    };
    crate::server::ensure_osv_allowed(state, tarball.name, &dist.version)?;
    state.inner.proxy.version_tgzs.invalidate_version(
        tarball.namespace,
        tarball.name,
        &dist.version,
    );
    timed(
        "tarball:upstream_fetch",
        tarball.name.as_str(),
        upstream.fetch_declared_tarball_response(tarball.name, tarball.filename, &dist.route),
    )
    .await
}

async fn recent_rotated_tarball_dist(
    state: &AppState,
    upstream: &Upstream,
    tarball: UpstreamTarball<'_>,
) -> Result<Option<TarballDist>, RegistryError> {
    if !upstream.caches() {
        return Ok(None);
    }
    let ttl = crate::server::upstream_packuments::REFETCH_INTERVAL;
    let Some(recent) =
        state.inner.storage.read_upstream_document(tarball.namespace, tarball.name, ttl).await?
    else {
        return Ok(None);
    };
    rotated_tarball_dist(state, recent, tarball).await
}

async fn rotated_tarball_dist(
    state: &AppState,
    packument: Vec<u8>,
    tarball: UpstreamTarball<'_>,
) -> Result<Option<TarballDist>, RegistryError> {
    Ok(parse_tarball_dist(state, packument, tarball.name, tarball.filename)
        .await?
        .filter(|dist| dist.integrity == *tarball.integrity && dist.route != *tarball.route))
}
