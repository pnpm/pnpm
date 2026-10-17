//! Which direct dependencies of an importer are injected.

use crate::{Lockfile, LockfileResolution, PkgName};
use pnpm_package_manifest::DependencyGroup;

impl Lockfile {
    /// The direct dependencies of `importer_id` that are injected: copies of
    /// a local directory, recorded as a `file:` version whose package entry
    /// has a `directory` resolution.
    ///
    /// The lockfile decides this, not the manifests. Whether a dependency is
    /// injected also depends on `injectWorkspacePackages`, `dependenciesMeta`,
    /// and the projects of the workspace, none of which a caller that sees
    /// only some of the projects can evaluate.
    pub fn injected_dependencies<'a>(
        &'a self,
        importer_id: &str,
    ) -> impl Iterator<Item = &'a PkgName> + 'a {
        let packages = self.packages.as_ref();
        self.importers
            .get(importer_id)
            .into_iter()
            .flat_map(|importer| {
                importer.dependencies_by_groups([
                    DependencyGroup::Prod,
                    DependencyGroup::Dev,
                    DependencyGroup::Optional,
                ])
            })
            .filter(move |(alias, spec)| {
                spec.version
                    .as_file_target()
                    .and_then(|_| spec.version.resolved_key(alias))
                    .and_then(|key| packages?.get(&key.without_peer()))
                    .is_some_and(|metadata| {
                        matches!(metadata.resolution, LockfileResolution::Directory(_))
                    })
            })
            .map(|(alias, _)| alias)
    }
}

#[cfg(test)]
mod tests;
