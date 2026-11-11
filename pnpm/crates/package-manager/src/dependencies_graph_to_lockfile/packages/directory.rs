use pnpm_lockfile::{LockfileResolution, PackageKey};
use pnpm_resolving_deps_resolver::DependenciesGraphNode;

pub(super) fn specifiers(
    node: &DependenciesGraphNode,
    key: &PackageKey,
    format: pnpm_lockfile::LockfileFormat,
) -> Result<Option<pnpm_lockfile::DeclaredSpecifiers>, super::super::DependenciesGraphToLockfileError>
{
    if format != pnpm_lockfile::LockfileFormat::V13
        || !matches!(node.resolve_result.resolution, LockfileResolution::Directory(_))
    {
        return Ok(None);
    }
    let manifest = node.resolve_result.package.manifest
        .as_deref()
        .ok_or_else(|| {
            super::super::DependenciesGraphToLockfileError::DirectoryManifestMissing {
                dep_path: node.dep_path.to_string(),
            }
        })?;
    let mut specifiers =
        pnpm_resolving_deps_resolver::declared_specifiers(manifest, &key.name.to_string())
            .map_err(|source| {
                super::super::DependenciesGraphToLockfileError::DirectorySpecifiers {
                    dep_path: node.dep_path.to_string(),
                    source: Box::new(source),
                }
            })?;
    specifiers.manifest_source = Some(if node.resolve_result.resolved_via == "local-filesystem" {
        pnpm_lockfile::DirectoryManifestSource::Path
    } else {
        pnpm_lockfile::DirectoryManifestSource::Workspace
    });
    Ok(Some(specifiers))
}
