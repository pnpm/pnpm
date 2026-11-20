use std::sync::Arc;

use pnpm_lockfile::{LockfileResolution, TarballResolution};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_tarball::{LocalTarballMetadata, TarballError, read_local_tarball_metadata};

use super::{
    LocalFilesystemPolicy, LocalResolveResult, LocalResolverContext, LocalResolverOptions,
    LocalResolverUpdate, LocalTarballResolution, ResolveLocalError,
};
use crate::parse_bare_specifier::LocalPackageSpec;

pub(super) async fn resolve_file_spec(
    spec: &LocalPackageSpec,
    opts: &LocalResolverOptions,
    ctx: LocalResolverContext,
) -> Result<LocalResolveResult, ResolveLocalError> {
    match resolve_local_tarball(spec, ctx).await {
        Ok(result) => Ok(result),
        Err(ResolveLocalError::LinkedPkgDirNotFound { .. })
            if reuse_current_tarball(spec, opts) =>
        {
            let current = opts.current_pkg.as_ref().expect("checked in reuse_current_tarball");
            Ok(LocalResolveResult {
                id: spec.id.clone(),
                manifest: current.manifest.as_ref().map(Arc::clone),
                normalized_bare_specifier: Some(spec.normalized_bare_specifier.clone()),
                resolution: current.resolution.clone(),
                resolved_via: spec.resolved_via,
            })
        }
        Err(err) => Err(err),
    }
}

fn reuse_current_tarball(spec: &LocalPackageSpec, opts: &LocalResolverOptions) -> bool {
    opts.update == LocalResolverUpdate::Off
        && opts.current_pkg
            .as_ref()
            .is_some_and(|current| {
                matches!(
                    &current.resolution,
                    LockfileResolution::Tarball(tarball)
                        if tarball.integrity.is_some()
                            && (tarball.tarball == spec.id.as_str()
                                || current.id.as_str() == spec.id.as_str()),
                )
            })
}

/// Resolve a `file:` specifier that names a tarball.
async fn resolve_local_tarball(
    spec: &LocalPackageSpec,
    ctx: LocalResolverContext,
) -> Result<LocalResolveResult, ResolveLocalError> {
    match ctx.filesystem_policy {
        LocalFilesystemPolicy::HostFilesystem(LocalTarballResolution::DeferToFetcher) => {
            return Ok(tarball_resolve_result(spec, None));
        }
        LocalFilesystemPolicy::HostFilesystem(LocalTarballResolution::ReadMetadata) => {}
        LocalFilesystemPolicy::WorkspaceOnly => {
            return Err(ResolveLocalError::LocalSpecNotAllowed {
                specifier: spec.normalized_bare_specifier.clone(),
            });
        }
    }
    // A missing tarball file raises the same `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND`
    // code the directory branch uses for a missing `file:` target, so both
    // kinds of missing `file:` target share one error code.
    let metadata =
        match read_local_tarball_metadata(&spec.fetch_spec, ctx.integrity_target.into()).await {
            Ok(metadata) => metadata,
            Err(err) if is_missing_tarball(&err) => {
                return Err(ResolveLocalError::LinkedPkgDirNotFound {
                    path: spec.fetch_spec.display().to_string(),
                });
            }
            Err(err) => return Err(ResolveLocalError::ReadTarball(err)),
        };
    if metadata.has_manifest_entry {
        check_bundled_package_name(metadata.manifest.as_ref(), &spec.normalized_bare_specifier)?;
    }
    Ok(tarball_resolve_result(spec, Some(metadata)))
}

fn tarball_resolve_result(
    spec: &LocalPackageSpec,
    metadata: Option<LocalTarballMetadata>,
) -> LocalResolveResult {
    let (manifest, integrity) = match metadata {
        Some(metadata) => (metadata.manifest.map(Arc::new), Some(metadata.integrity)),
        None => (None, None),
    };
    LocalResolveResult {
        id: spec.id.clone(),
        manifest,
        normalized_bare_specifier: Some(spec.normalized_bare_specifier.clone()),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: spec.id.as_str().to_string(),
            integrity,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: spec.resolved_via,
    }
}

/// The bundled name prefixes the dep path and names the package's
/// `node_modules` directory, so it has to be present and valid before it
/// reaches either. An archive that ships no manifest at all is a different
/// shape and stays tolerated by the caller.
fn check_bundled_package_name(
    manifest: Option<&serde_json::Value>,
    specifier: &str,
) -> Result<(), ResolveLocalError> {
    let Some(name) = bundled_package_name(manifest) else {
        return Err(ResolveLocalError::MissingPackageName { specifier: specifier.to_string() });
    };
    if is_valid_old_npm_package_name(name) {
        return Ok(());
    }
    Err(ResolveLocalError::InvalidPackageName {
        specifier: specifier.to_string(),
        name: name.to_string(),
    })
}

fn bundled_package_name(manifest: Option<&serde_json::Value>) -> Option<&str> {
    manifest?
        .get("name")?
        .as_str()
        .filter(|name| !name.is_empty())
}

fn is_missing_tarball(err: &TarballError) -> bool {
    matches!(err, TarballError::ReadLocalTarball { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
}
