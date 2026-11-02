//! Platform variant routes (see [`super::super::publishing::variants`]).

use super::{
    AppState, AuthedCaller, Deserialize, HeaderMap, Path, Response, State, TargetRegistry,
    not_found, npm_handlers::ScopedVersionPath, serve_version_manifest, split_package_identifier,
};

#[derive(Deserialize)]
pub(super) struct ScopedVariantPath {
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) tag: String,
}

/// `GET {base}/@{scope}/{pkg}/{version-or-tag}`, or
/// `GET {base}/{pkg}@{version}/variants/{tag}` — an unscoped package's
/// platform variant, which has the same shape.
pub(super) async fn get_scoped_version_manifest_or_variant(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(path): Path<ScopedVersionPath>,
) -> Response {
    if path.name == "variants"
        && let Some((package, package_version)) = split_package_identifier(&path.scope)
    {
        return super::super::variant_tarballs::serve_variant_tarball(
            &state,
            &identity,
            &headers,
            registry.as_deref(),
            (package, package_version, &path.version),
        )
        .await;
    }
    // A scoped package's version manifest. A first segment that is not a
    // scope names no package.
    let ScopedVersionPath { scope, name, version } = path;
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_version_manifest(&state, &identity, registry.as_deref(), &full, &version).await
}

/// `GET {base}/@{scope}/{pkg}@{version}/variants/{tag}` — a scoped package's
/// platform variant.
pub(super) async fn get_scoped_variant_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(path): Path<ScopedVariantPath>,
) -> Response {
    let ScopedVariantPath { scope, name, tag } = path;
    let Some((bare_name, version)) = split_package_identifier(&name) else {
        return not_found();
    };
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{bare_name}");
    super::super::variant_tarballs::serve_variant_tarball(
        &state,
        &identity,
        &headers,
        registry.as_deref(),
        (&full, version, &tag),
    )
    .await
}
