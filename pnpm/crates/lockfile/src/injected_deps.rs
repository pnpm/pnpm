//! Which direct dependencies of an importer are injected.

use crate::{DirectoryResolution, Lockfile, LockfileResolution, PkgName, ResolvedDependencySpec};
use pnpm_package_manifest::DependencyGroup;

impl Lockfile {
    /// The direct dependencies of `importer_id` in `groups` that are
    /// injected: copies of a local directory, whose package entry has a
    /// `directory` resolution. The importer records one as a `file:`
    /// version, or as `<name>@file:<path>` when the alias renames it.
    ///
    /// The lockfile decides this, not the manifests. Whether a dependency is
    /// injected also depends on `injectWorkspacePackages`, `dependenciesMeta`,
    /// and the projects of the workspace, none of which a caller that sees
    /// only some of the projects can evaluate.
    pub fn injected_dependencies<'a>(
        &'a self,
        importer_id: &str,
        groups: impl IntoIterator<Item = DependencyGroup> + 'a,
    ) -> impl Iterator<Item = InjectedDependency<'a>> + 'a {
        let packages = self.packages.as_ref();
        self.importers
            .get(importer_id)
            .map(|importer| importer.dependencies_by_groups(groups))
            .into_iter()
            .flatten()
            .filter_map(move |(alias, spec)| {
                let metadata = packages?.get(&spec.version.resolved_key(alias)?.without_peer())?;
                match &metadata.resolution {
                    LockfileResolution::Directory(directory) => {
                        Some(InjectedDependency { alias, spec, directory })
                    }
                    _ => None,
                }
            })
    }
}

/// One entry of [`Lockfile::injected_dependencies`].
pub struct InjectedDependency<'a> {
    pub alias: &'a PkgName,
    pub spec: &'a ResolvedDependencySpec,
    pub directory: &'a DirectoryResolution,
}

#[cfg(test)]
mod tests;
