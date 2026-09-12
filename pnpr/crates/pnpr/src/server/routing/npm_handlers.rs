use super::{
    AppState, AuthedCaller, Deserialize, Ecosystem, HeaderMap, Path, RawQuery, Response, State,
    TargetRegistry, addressed_registry, caller_scoped, delete_package, delete_tarball,
    get_dist_tags, not_found, private_no_cache, publish_package, remove_dist_tag, serve_packument,
    serve_revision_tarball, serve_search, serve_tarball, serve_version_manifest, set_dist_tag,
    update_packument,
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
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref(), Ecosystem::Npm) else {
        return not_found();
    };
    serve_revision_tarball(&state, &identity, &target, &path.digest).await
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

/// `PUT {base}/{pkg}`.
pub(super) async fn put_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
    body: axum::body::Bytes,
) -> Response {
    publish_package(&state, &identity, registry.as_deref(), &path.name, body).await
}

/// `PUT {base}/@{scope}/{pkg}` — publish a scoped package. A first segment
/// that is not a scope names no publishable package.
pub(super) async fn put_scoped_package(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TwoSegments>,
    body: axum::body::Bytes,
) -> Response {
    let TwoSegments { first, second } = path;
    if !first.starts_with('@') {
        return not_found();
    }
    let full = format!("{first}/{second}");
    publish_package(&state, &identity, registry.as_deref(), &full, body).await
}

/// `PUT {base}/{pkg}/-rev/{rev}` — the full mutated packument, which is how
/// npm spells a partial unpublish.
pub(super) async fn put_packument_revision(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
    body: axum::body::Bytes,
) -> Response {
    update_packument(&state, &identity, registry.as_deref(), &path.name, &body).await
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
