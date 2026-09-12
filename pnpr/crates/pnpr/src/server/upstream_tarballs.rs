use super::{
    AppState, CanonicalPackageName, Duration, FetchOutcome, Identity, Integrity, MAX_TARBALL_BYTES,
    RegistryError, Response, TarballDist, Upstream, authorized_upstream, ensure_osv_allowed,
    expected_tarball_dist, load_upstream_packument, not_found, streaming, tarball_response,
    tarball_stream_error, timed, upstream_cache_namespace,
};
use axum::response::IntoResponse;

/// Serve a tarball through an upstream's `/~<name>/` endpoint. The version's
/// `dist.integrity` is read from the upstream's own packument (served from the
/// private cache when fresh), and the bytes are verified against it. Both the
/// packument and the verified tarball are cached under the upstream's private
/// namespace, so a private upstream's content never lands in the shared proxy
/// mirror yet is not re-fetched on every request.
pub(super) async fn serve_tarball_via_upstream(
    state: &AppState,
    identity: &Identity,
    upstream: &str,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let (filename, parsed_version) = match tarball_cache_name(&name, filename) {
        Ok(named) => named,
        Err(err) => return err.into_response(),
    };
    let namespace = upstream_cache_namespace(state, upstream);
    let upstream = match authorized_upstream(state, identity, upstream) {
        Ok(upstream) => upstream,
        Err(err) => return err.into_response(),
    };
    serve_authorized_upstream_tarball(
        state,
        upstream,
        &namespace,
        &name,
        &filename,
        parsed_version.as_deref(),
    )
    .await
}

pub(super) async fn serve_authorized_upstream_tarball(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    name: &CanonicalPackageName,
    filename: &str,
    parsed_version: Option<&str>,
) -> Response {
    // Pre-check OSV on the filename-derived version (when the name is
    // canonical) to fail fast; the authoritative check against the
    // packument-resolved version runs below either way.
    if let Err(err) = screen_parsed_version(state, name, parsed_version) {
        return err.into_response();
    }
    let ttl = upstream.maxage().unwrap_or(state.inner.config.packument_ttl);
    // Serve a cached hit before touching the packument: a cached entry was
    // bound to a declared version and verified against `dist.integrity` when
    // it was written, and the client re-verifies what it receives, so no
    // re-bind or re-hash is needed. The packument load — and the full-document
    // JSON parse in `expected_tarball_dist` — costs milliseconds per request
    // for a large package and would dominate warm tarball serving.
    //
    // Deliberately, a hit is NOT re-bound against the packument as it stands
    // *now*: a version unpublished since the write stays downloadable from
    // this disposable mirror until the entry is wiped (registry-CDN
    // semantics; resolution already stops offering it once the refreshed
    // packument drops it), and a hostile packument rewrite — say, duplicate
    // `dist.tarball` basenames — cannot retroactively poison bytes that were
    // verified on the way in. The fail-closed bind below protects the *fetch*
    // of new bytes; end-to-end SRI (the client's lockfile) is the authority
    // on what it accepts. Only OSV screening needs the packument-resolved
    // version first, so with OSV enabled the cache read waits for the bind
    // below. A `cache: false` upstream skips the cache and streams through.
    if upstream.caches()
        && state.inner.osv_index.is_none()
        && let Some(response) = cached_upstream_tarball(state, namespace, name, filename).await
    {
        return response;
    }
    let dist = bind_tarball_to_packument(state, upstream, namespace, name, filename, ttl).await;
    let TarballDist { version, integrity } = match dist {
        Ok(Some(dist)) => dist,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    if let Err(err) = recheck_osv(state, name, &version, parsed_version) {
        return err.into_response();
    }
    if upstream.caches()
        && state.inner.osv_index.is_some()
        && let Some(response) = cached_upstream_tarball(state, namespace, name, filename).await
    {
        return response;
    }

    fetch_upstream_tarball(
        state,
        upstream,
        UpstreamTarball { namespace, name, filename, integrity: &integrity },
    )
    .await
}

/// The cache path segment a tarball request names, and the version its
/// filename declares when it is canonical.
///
/// A canonical `<basename>-<version>.tgz` (or the scoped wire form) is
/// normalized as usual. A non-canonical basename preserved verbatim from the
/// upstream's `dist.tarball` (see `pnpr_upstream::rewrite_tarball_urls`) is
/// accepted opaquely so long as it is safe as a cache path segment — the
/// packument match is what authorizes it, binding it to a declared version and
/// integrity. Rejecting it here would make such a version un-fetchable through
/// the very URL this server advertised.
pub(super) fn tarball_cache_name(
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<(String, Option<String>), RegistryError> {
    match name.parse_tarball_name(filename) {
        Ok((canonical, version)) => Ok((canonical, Some(version))),
        Err(_) if pnpr_package_name::is_safe_path_segment(filename) => {
            Ok((filename.to_string(), None))
        }
        Err(err) => Err(err),
    }
}

/// Bind a tarball request to the version and integrity the upstream's own
/// packument declares for it. `None` means the packument names no such
/// tarball.
pub(super) async fn bind_tarball_to_packument(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    name: &CanonicalPackageName,
    filename: &str,
    ttl: Duration,
) -> Result<Option<TarballDist>, RegistryError> {
    let packument = timed(
        "tarball:packument_load",
        name.as_str(),
        load_upstream_packument(state, namespace, upstream, name, ttl),
    )
    .await?;
    let Some(packument) = packument else {
        return Ok(None);
    };
    expected_tarball_dist(&packument, name, filename)
}

/// Screen the version a canonical filename declares, so an advisory-blocked
/// version fails before the packument is loaded.
pub(super) fn screen_parsed_version(
    state: &AppState,
    name: &CanonicalPackageName,
    parsed_version: Option<&str>,
) -> Result<(), RegistryError> {
    let Some(version) = parsed_version else {
        return Ok(());
    };
    ensure_osv_allowed(state, name, version)
}

/// Screen the packument-resolved version when the filename declared a
/// different one; the filename's own version was already screened.
pub(super) fn recheck_osv(
    state: &AppState,
    name: &CanonicalPackageName,
    version: &str,
    parsed_version: Option<&str>,
) -> Result<(), RegistryError> {
    if parsed_version == Some(version) {
        return Ok(());
    }
    ensure_osv_allowed(state, name, version)
}

/// The tarball an upstream fetch is about to stream.
pub(super) struct UpstreamTarball<'a> {
    pub(super) namespace: &'a str,
    pub(super) name: &'a CanonicalPackageName,
    pub(super) filename: &'a str,
    pub(super) integrity: &'a Integrity,
}

/// Fetch the tarball from the upstream and stream it to the client, teeing it
/// into the namespaced cache when the upstream is cacheable.
pub(super) async fn fetch_upstream_tarball(
    state: &AppState,
    upstream: &Upstream,
    tarball: UpstreamTarball<'_>,
) -> Response {
    let UpstreamTarball { namespace, name, filename, integrity } = tarball;
    let fetched = timed(
        "tarball:upstream_fetch",
        name.as_str(),
        upstream.fetch_tarball_response(name, filename),
    )
    .await;
    let response = match fetched {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let write = match state.inner.storage.open_upstream_blob_tmp(namespace, name, filename).await {
        Ok(write) => write,
        Err(err) => return err.into_response(),
    };
    if !upstream.caches() {
        return stream_verified_without_caching(response, write, integrity, name, filename).await;
    }
    // The entry is promoted only on an SRI match (see
    // `stream_verified_to_cache`). No `Content-Length` is set: the upstream's
    // is attacker-controlled and unverifiable before streaming, so the body is
    // chunked and the client reads to EOF (then re-verifies the integrity).
    match streaming::stream_verified_to_cache(response, write, integrity, MAX_TARBALL_BYTES) {
        Ok(body) => tarball_response(body, None),
        Err(err) => tarball_stream_error(err, name, filename).into_response(),
    }
}

/// Fetch-through: verify and stream from the temp file, then remove it, so a
/// `cache: false` upstream's tarball is never persisted.
pub(super) async fn stream_verified_without_caching(
    response: pnpm_network::ThrottledResponse,
    write: pnpr_storage::BlobWrite,
    integrity: &ssri::Integrity,
    name: &CanonicalPackageName,
    filename: &str,
) -> Response {
    let downloaded =
        streaming::download_verified_to_temp(response, write, integrity, MAX_TARBALL_BYTES).await;
    match downloaded {
        Ok((file, len, tmp_path)) => {
            tarball_response(streaming::stream_file_and_remove(file, tmp_path), Some(len))
        }
        Err(err) => tarball_stream_error(err, name, filename).into_response(),
    }
}

/// The response for a cached upstream tarball, or `None` on a cache miss. A
/// cache-open fault is logged and treated as a miss so the caller falls back
/// to the upstream fetch rather than failing the request.
pub(super) async fn cached_upstream_tarball(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    filename: &str,
) -> Option<Response> {
    match timed(
        "tarball:cache_read",
        name.as_str(),
        state.inner.storage.open_upstream_blob(namespace, name, filename),
    )
    .await
    {
        Ok(Some((file, len))) => Some(tarball_response(streaming::stream_file(file), Some(len))),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(?err, package = %name.as_str(), %filename, "upstream tarball cache open failed");
            None
        }
    }
}
