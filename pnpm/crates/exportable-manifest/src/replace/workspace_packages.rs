//! The workspace's packages, and the project a `workspace:` specifier selects
//! among several of one name.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// The workspace's packages by name. A name holds one entry per project that
/// carries it: a workspace may hold several versions of one package.
pub type WorkspacePackageManifests = HashMap<String, Vec<WorkspacePackageManifest>>;

/// The fields the rewriters consult on the dependency's manifest, and the
/// registry prefix the dependency publishes under. `version` is empty when a
/// workspace package was found but its `package.json` has no `version` field,
/// so the caller can report that instead of claiming the dependency is not
/// installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePackageManifest {
    pub name: String,
    pub version: String,
    /// The workspace project's directory. `None` for a manifest read from a
    /// dependency's directory.
    pub root_dir: Option<PathBuf>,
    /// The named-registry prefix of the registry the package publishes to,
    /// when the workspace declares one for that registry. A published
    /// specifier for the package carries it (`work:1.0.0`), so a consumer
    /// records one lockfile key for the package whether it addresses the
    /// package through the prefix or through this manifest. `None` leaves the
    /// specifier plain.
    pub registry_prefix: Option<String>,
}

impl WorkspacePackageManifest {
    pub(super) fn is_complete(&self) -> bool {
        !self.name.is_empty() && !self.version.is_empty()
    }

    /// A manifest read from a dependency's directory says nothing about
    /// registries; the prefix is that of the workspace project in
    /// `dependency_dir`, else of the project of the same name and version.
    pub(super) fn with_registry_prefix_of(
        mut self,
        dependency_dir: &Path,
        workspace_packages: Option<&WorkspacePackageManifests>,
    ) -> Self {
        self.registry_prefix = workspace_package_in(workspace_packages, dependency_dir, &self.name)
            .or_else(|| workspace_package_at(workspace_packages, &self.name, &self.version))
            .and_then(|project| project.registry_prefix.clone());
        self
    }
}

/// The workspace package `name` at `version`, or the one project of that name
/// when `version` is none of them. Several projects of the name and no match
/// select nothing: a guess would send a consumer to another project's
/// registry.
pub(super) fn workspace_package_at<'a>(
    workspace_packages: Option<&'a WorkspacePackageManifests>,
    name: &str,
    version: &str,
) -> Option<&'a WorkspacePackageManifest> {
    let projects = workspace_packages?.get(name)?;
    if let Some(project) = projects
        .iter()
        .find(|project| project.version == version)
    {
        return Some(project);
    }
    match projects.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

/// The workspace project of `name` whose directory `dependency_dir` resolves
/// to, through a `node_modules` symlink or a relative path. Only consulted
/// when several projects carry the name, since they may share a version.
fn workspace_package_in<'a>(
    workspace_packages: Option<&'a WorkspacePackageManifests>,
    dependency_dir: &Path,
    name: &str,
) -> Option<&'a WorkspacePackageManifest> {
    let projects = workspace_packages?.get(name)?;
    if projects.len() < 2 {
        return None;
    }
    let target = fs::canonicalize(dependency_dir).ok()?;
    projects
        .iter()
        .find(|project| {
            project.root_dir
                .as_deref()
                .and_then(|root_dir| fs::canonicalize(root_dir).ok())
                .is_some_and(|root_dir| root_dir == target)
        })
}

/// The workspace package a `workspace:` specifier falls back on when the
/// dependency is not installed: the first project of the name with a version,
/// else the first, so that a missing version can be reported.
pub(super) fn workspace_package_named<'a>(
    workspace_packages: Option<&'a WorkspacePackageManifests>,
    name: &str,
) -> Option<&'a WorkspacePackageManifest> {
    let projects = workspace_packages?.get(name)?;
    projects
        .iter()
        .find(|project| project.is_complete())
        .or_else(|| projects.first())
}
