pub(crate) use importer_deps::check_directory_dependencies_freshness;
pub use injected_manifest::ProjectManifestsByDir;
pub(super) use injected_manifest::{InjectedDirectory, is_deploy_file_url_key, unhooked_manifest};
pub(crate) use injected_manifest::{dependency_manifests_by_dir, project_manifests_by_dir};
pub(crate) use spec::spec_target_and_range;

mod children;
mod importer_deps;
mod injected_manifest;
mod package;
mod peers;
mod scope;
mod spec;

#[cfg(test)]
mod tests;

use super::manifest::ImporterSatisfactionCheck;
use crate::install::lockfile_freshness::FreshnessCheckError;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::StalenessReason;
use spec::SpecDirs;
use std::path::Path;

struct LocalDepContext<'a> {
    name: &'a str,
    /// The importer declared the dependency with a `file:` spec, which the
    /// resolver reads from the directory itself rather than from a workspace
    /// project.
    declared_as_file: bool,
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
            declared_as_file: is_declared_local_directory(spec),
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

pub(crate) fn is_declared_local_directory(spec: &str) -> bool {
    let Some(path) = spec.strip_prefix("file:") else {
        return false;
    };
    !pnpm_lockfile::is_local_tarball_path(path)
}

fn spec_dirs<'a>(check: &'a ImporterSatisfactionCheck<'a>) -> SpecDirs<'a> {
    SpecDirs {
        workspace_root: check.config.workspace_dir.as_deref().unwrap_or(check.lockfile_dir),
        lockfile_dir: check.lockfile_dir,
        manifests_by_dir: check.workspace.manifests_by_dir,
        canonical_paths: check.workspace.canonical_paths,
    }
}
