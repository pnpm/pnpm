use pnpm_lockfile::{LockfileResolution, PackageKey};
use pnpm_resolving_deps_resolver::DependenciesGraphNode;

pub(super) fn specifiers(
    node: &DependenciesGraphNode,
    key: &PackageKey,
    format: pnpm_lockfile::LockfileFormat,
) -> Option<pnpm_lockfile::DeclaredSpecifiers> {
    if format != pnpm_lockfile::LockfileFormat::V13
        || !matches!(node.resolve_result.resolution, LockfileResolution::Directory(_))
    {
        return None;
    }
    let manifest = node.resolve_result.package.manifest
        .as_deref()
        .expect("a resolved directory carries its manifest");
    let mut specifiers =
        pnpm_resolving_deps_resolver::declared_specifiers(manifest, &key.name.to_string())
            .expect("the resolver validated these directory dependency specifiers");
    specifiers.manifest_source = Some(if node.resolve_result.resolved_via == "local-filesystem" {
        pnpm_lockfile::DirectoryManifestSource::Path
    } else {
        pnpm_lockfile::DirectoryManifestSource::Workspace
    });
    Some(specifiers)
}
