//! Which direct dependencies of an importer are injected.

use crate::{Lockfile, LockfileResolution, PkgName};
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
    ) -> impl Iterator<Item = &'a PkgName> + 'a {
        let packages = self.packages.as_ref();
        self.importers
            .get(importer_id)
            .map(|importer| importer.dependencies_by_groups(groups))
            .into_iter()
            .flatten()
            .filter(move |(alias, spec)| {
                spec.version
                    .resolved_key(alias)
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
