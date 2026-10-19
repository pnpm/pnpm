use pnpr_policy::Identity;

use super::{
    Action, AppState, AuthedCaller, CanonicalPackageName, Deserialize, Ecosystem, HeaderMap,
    IntoResponse, MAX_PUBLISH_BODY_BYTES, Path, RawQuery, RegistrySource, Response, State,
    StatusCode, TargetRegistry, addressed_registry, authorize_package, caller_scoped,
    check_publish_tombstone, delete_package, delete_tarball, get_dist_tags, hosted_storage,
    not_found, private_no_cache, publish_package, remove_dist_tag, resolve_write_target,
    serve_package_tarball, serve_packument, serve_revision_tarball, serve_search, serve_tarball,
    serve_version_manifest, set_dist_tag, split_package_identifier, update_packument,
};

// --------------------------------------------------------------------
// Path shapes. Each names only the segments its handlers read: the
// `/~<name>/` registration captures a `registry` segment too, and the
// `-rev` routes capture a revision token pnpr does not track.
// --------------------------------------------------------------------

/// A package addressed by a single segment: an unscoped name, or a scoped name
/// percent-encoded as `@scope%2Fname`.
#[derive(Deserialize)]
pub(super) struct NamePath {
    pub(super) name: String,
}

/// npm's overloaded two-segment address. Which package it names, and which
/// resource of it, depends on the first segment's shape and on the method, so
/// neither segment can be given a resource name here.
#[derive(Deserialize)]
pub(super) struct TwoSegments {
    pub(super) first: String,
    pub(super) second: String,
}

#[derive(Deserialize)]
pub(super) struct ScopedVersionPath {
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) version: String,
}

#[derive(Deserialize)]
pub(super) struct TarballPath {
    pub(super) name: String,
    pub(super) filename: String,
}

#[derive(Deserialize)]
pub(super) struct ScopedTarballPath {
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) filename: String,
}

#[derive(Deserialize)]
pub(super) struct ScopedRevPath {
    pub(super) scope: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) _rev: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct DistTagPath {
    pub(super) name: String,
    pub(super) tag: String,
}

#[derive(Deserialize)]
pub(super) struct DigestPath {
    pub(super) digest: String,
}

// --------------------------------------------------------------------
// Package reads — packument, version manifest, tarball.
// --------------------------------------------------------------------

/// `GET {base}/{pkg}`.
pub(super) async fn get_packument(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(path): Path<NamePath>,
) -> Response {
    if let Some((name, version)) = split_package_identifier(&path.name) {
        return serve_package_tarball(
            &state,
            &identity,
            &headers,
            registry.as_deref(),
            (name, version),
        )
        .await;
    }
    serve_packument(&state, &identity, &headers, registry.as_deref(), &path.name).await
}

/// `GET {base}/@{scope}/{pkg}` — a scoped package's packument — or
/// `GET {base}/{pkg}/{version-or-tag}` — a version manifest for a package
/// whose name fits one segment.
pub(super) async fn get_packument_or_version_manifest(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(path): Path<TwoSegments>,
) -> Response {
    let TwoSegments { first, second } = path;
    if first.starts_with('@') && !first.contains('/') {
        if let Some((bare_name, version)) = split_package_identifier(&second) {
            let name = format!("{first}/{bare_name}");
            return serve_package_tarball(
                &state,
                &identity,
                &headers,
                registry.as_deref(),
                (&name, version),
            )
            .await;
        }
        let name = format!("{first}/{second}");
        return serve_packument(&state, &identity, &headers, registry.as_deref(), &name).await;
    }
    serve_version_manifest(&state, &identity, registry.as_deref(), &first, &second).await
}

/// `GET {base}/@{scope}/{pkg}/{version-or-tag}` — a scoped package's version
/// manifest. A first segment that is not a scope names no package.
pub(super) async fn get_scoped_version_manifest(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedVersionPath>,
) -> Response {
    let ScopedVersionPath { scope, name, version } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_version_manifest(&state, &identity, registry.as_deref(), &full, &version).await
}

/// `GET {base}/{pkg}/-/{filename}`.
pub(super) async fn get_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TarballPath>,
) -> Response {
    serve_tarball(&state, &identity, registry.as_deref(), &path.name, &path.filename).await
}

/// `GET {base}/@{scope}/{pkg}/-/{filename}`.
pub(super) async fn get_scoped_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedTarballPath>,
) -> Response {
    let ScopedTarballPath { scope, name, filename } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &identity, registry.as_deref(), &full, &filename).await
}

/// `GET {base}/-/tarballs/sha512/{digest}` — an integrity-addressed tarball.
pub(super) async fn get_revision_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<DigestPath>,
    headers: HeaderMap,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref(), Ecosystem::Npm) else {
        return not_found();
    };
    serve_revision_tarball(&state, &identity, &headers, &target, &path.digest).await
}

/// `GET {base}/-/v1/search`.
pub(super) async fn get_search(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.unwrap_or_default();
    // Results are filtered per caller (registry access plus per-package ACL),
    // so they must never land in a shared HTTP cache.
    private_no_cache(serve_search(&state, &identity, registry.as_deref(), &query).await)
}

// --------------------------------------------------------------------
// Package writes — publish, unpublish, dist-tags.
// --------------------------------------------------------------------

async fn preauthorize_publish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Result<CanonicalPackageName, Response> {
    let name = match CanonicalPackageName::parse(raw_name, Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return Err(err.into_response()),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return Err(err.into_response()),
    };
    if let Err(err) = authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
        Some(&target.org),
    )
    .await
    {
        return Err(err.into_response());
    }
    let storage = hosted_storage(state, Some(&target.org));
    check_publish_tombstone(state, identity, &storage, &name).await
        .map_err(IntoResponse::into_response)?;
    Ok(name)
}

async fn preauthorize_rewrite(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Result<CanonicalPackageName, Response> {
    let name = match CanonicalPackageName::parse(raw_name, Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return Err(err.into_response()),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return Err(err.into_response()),
    };
    let source = RegistrySource::Hosted(target.source);
    if let Err(err) = authorize_package(
        state,
        identity,
        &source,
        name.as_str(),
        Action::Publish,
        Some(&target.org),
    )
    .await
    {
        return Err(err.into_response());
    }
    Ok(name)
}

async fn collect_publish_body(
    state: &AppState,
    body: axum::body::Body,
) -> Result<axum::body::Bytes, Response> {
    let _permit = state.inner.locks.publish_concurrency.acquire().await;
    axum::body::to_bytes(body, MAX_PUBLISH_BODY_BYTES).await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE.into_response())
}

/// `PUT {base}/{pkg}`.
pub(super) async fn put_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
    body: axum::body::Body,
) -> Response {
    if let Err(rejection) =
        preauthorize_publish(&state, &identity, registry.as_deref(), &path.name).await
    {
        return rejection;
    }
    let body_bytes = match collect_publish_body(&state, body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    publish_package(&state, &identity, registry.as_deref(), &path.name, body_bytes).await
}

/// `PUT {base}/@{scope}/{pkg}` — publish a scoped package. A first segment
/// that is not a scope names no publishable package.
pub(super) async fn put_scoped_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TwoSegments>,
    body: axum::body::Body,
) -> Response {
    let TwoSegments { first, second } = path;
    if !first.starts_with('@') {
        return not_found();
    }
    let full = format!("{first}/{second}");
    if let Err(rejection) =
        preauthorize_publish(&state, &identity, registry.as_deref(), &full).await
    {
        return rejection;
    }
    let body_bytes = match collect_publish_body(&state, body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    publish_package(&state, &identity, registry.as_deref(), &full, body_bytes).await
}

/// `PUT {base}/{pkg}/-rev/{rev}` — the full mutated packument, which is how
/// npm spells a partial unpublish.
pub(super) async fn put_packument_revision(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
    body: axum::body::Body,
) -> Response {
    if let Err(rejection) =
        preauthorize_rewrite(&state, &identity, registry.as_deref(), &path.name).await
    {
        return rejection;
    }
    let body_bytes = match collect_publish_body(&state, body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    update_packument(&state, &identity, registry.as_deref(), &path.name, &body_bytes).await
}

/// `PUT {base}/@{scope}/{pkg}/-rev/{rev}` — full mutated packument or
/// maintainer update for a scoped package.
pub(super) async fn put_scoped_packument_revision(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedRevPath>,
    body: axum::body::Body,
) -> Response {
    let ScopedRevPath { scope, name, .. } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    if let Err(rejection) =
        preauthorize_rewrite(&state, &identity, registry.as_deref(), &full).await
    {
        return rejection;
    }
    let body_bytes = match collect_publish_body(&state, body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    update_packument(&state, &identity, registry.as_deref(), &full, &body_bytes).await
}

/// `DELETE {base}/{pkg}/-rev/{rev}` — remove a whole package
/// (`pnpm unpublish --force`).
pub(super) async fn unpublish_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
) -> Response {
    delete_package(&state, &identity, registry.as_deref(), &path.name).await
}

/// `DELETE {base}/@{scope}/{pkg}/-rev/{rev}` — remove a whole scoped package.
pub(super) async fn unpublish_scoped_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedRevPath>,
) -> Response {
    let ScopedRevPath { scope, name, .. } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    delete_package(&state, &identity, registry.as_deref(), &full).await
}

/// `DELETE {base}/{pkg}/-/{filename}/-rev/{rev}` — remove one version's
/// tarball, a step of `pnpm unpublish <pkg>@<version>`.
pub(super) async fn unpublish_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TarballPath>,
) -> Response {
    delete_tarball(&state, &identity, registry.as_deref(), &path.name, &path.filename).await
}

/// `DELETE {base}/@{scope}/{pkg}/-/{filename}/-rev/{rev}` — remove one scoped
/// version's tarball. The unpublish flow reconstructs this URL from the
/// packument's `dist.tarball`, which spells a scoped name with a literal
/// slash.
pub(super) async fn unpublish_scoped_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedTarballPath>,
) -> Response {
    let ScopedTarballPath { scope, name, filename } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    delete_tarball(&state, &identity, registry.as_deref(), &full, &filename).await
}

/// `GET {base}/-/package/{pkg}/dist-tags`.
pub(super) async fn get_package_dist_tags(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
) -> Response {
    let response = get_dist_tags(&state, &identity, registry.as_deref(), &path.name).await;
    caller_scoped(&state, Ecosystem::Npm, registry.as_deref(), Some(&path.name), response)
}

/// `PUT {base}/-/package/{pkg}/dist-tags/{tag}`.
pub(super) async fn put_package_dist_tag(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<DistTagPath>,
    body: axum::body::Bytes,
) -> Response {
    set_dist_tag(&state, &identity, registry.as_deref(), &path.name, &path.tag, &body).await
}

/// `DELETE {base}/-/package/{pkg}/dist-tags/{tag}`.
pub(super) async fn delete_package_dist_tag(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<DistTagPath>,
) -> Response {
    remove_dist_tag(&state, &identity, registry.as_deref(), &path.name, &path.tag).await
}
