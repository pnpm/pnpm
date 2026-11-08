pub(crate) use crate::install_state::ProjectManifestsByDir;
pub(crate) use importer_deps::check_directory_dependencies_freshness;
pub(super) use injected_manifest::{InjectedDirectory, is_deploy_file_url_key, unhooked_manifest};
pub(crate) use injected_manifest::{dependency_manifests_by_dir, project_manifests_by_dir};
pub(crate) use nested::{
    GraphEdges, NestedByImporter, NestedDirectoryPackage, check_nested_directory_package,
    nested_directory_packages,
};
pub(super) use package::overridden_unhooked_manifest;
pub(crate) use spec::spec_target_and_range;

mod children;
mod importer_deps;
mod injected_manifest;
mod nested;
mod package;
mod peers;
mod scope;
mod spec;

#[cfg(test)]
mod tests;

use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use injected_manifest::is_deploy_file_url;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::StalenessReason;
use spec::SpecDirs;
use std::path::Path;

struct LocalDepContext<'a> {
    name: &'a str,
    /// See [`is_declared_by_path`].
    declared_by_path: bool,
    /// The lockfile does not record which spec declared the package, so it
    /// is checked under both declarations. Only the workspace one judges its
    /// `catalog:` children then.
    declaration_assumed: bool,
    rel_path: &'a str,
    dir: &'a Path,
    dirs: SpecDirs<'a>,
    catalogs: &'a Catalogs,
}

impl<'a> LocalDepContext<'a> {
    fn outdated(&self) -> FreshnessCheckError {
        local_dependency_outdated(self.name, self.rel_path)
    }

    fn child(&self, name: &'a str, spec: &str, rel_path: &'a str, dir: &'a Path) -> Self {
        Self {
            name,
            declared_by_path: is_declared_by_path(spec),
            declaration_assumed: false,
            rel_path,
            dir,
            dirs: self.dirs,
            catalogs: self.catalogs,
        }
    }
}

fn local_dependency_outdated(name: &str, path: &str) -> FreshnessCheckError {
    FreshnessCheckError::Stale(StalenessReason::LocalDependencyOutdated {
        name: name.to_string(),
        path: path.to_string(),
    })
}

/// Whether the local resolver claims `spec` as a filesystem path, a `file:`
/// spec or a bare one such as `../lib`. It resolves such a directory as
/// `local-filesystem`, not through the workspace: the directory's children
/// skip the catalogs, and its manifest is read from the directory itself.
pub(crate) fn is_declared_by_path(spec: &str) -> bool {
    pnpm_local_spec::is_local_filesystem_specifier(spec)
}

pub(crate) fn is_declared_local_directory(spec: &str) -> bool {
    let Some(path) = spec.strip_prefix("file:") else {
        return false;
    };
    !pnpm_local_spec::is_tarball_filename(path)
}

fn spec_dirs<'a>(check: &'a ImporterSatisfactionCheck<'a>) -> SpecDirs<'a> {
    SpecDirs {
        workspace_root: check.config.workspace_dir.as_deref().unwrap_or(check.lockfile_dir),
        lockfile_dir: check.lockfile_dir,
        manifests_by_dir: check.workspace.manifests_by_dir,
        canonical_paths: check.workspace.canonical_paths,
        registries: check.workspace.registries,
    }
}
