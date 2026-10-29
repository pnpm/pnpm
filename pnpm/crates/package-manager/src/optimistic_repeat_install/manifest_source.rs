//! How the repeat-install check reads the project manifests it is handed.

use crate::install::ProjectManifestsByDir;
use pnpm_package_manifest::PackageManifest;
use std::path::PathBuf;

/// The project manifests as the projects declare them: read from disk, or as
/// a mutating command writes them after the install. The workspace state
/// records their digests and the next check compares those with the files,
/// so a view that `packageExtensions` or a `readPackage` hook rewrote must
/// never stand in for them.
#[derive(Clone, Copy)]
pub struct DeclaredManifests<'a>(&'a [(PathBuf, &'a PackageManifest)]);

impl<'a> DeclaredManifests<'a> {
    #[must_use]
    pub fn new(manifests: &'a [(PathBuf, &'a PackageManifest)]) -> Self {
        Self(manifests)
    }

    #[must_use]
    pub fn as_slice(self) -> &'a [(PathBuf, &'a PackageManifest)] {
        self.0
    }
}

impl<'a> std::ops::Deref for DeclaredManifests<'a> {
    type Target = [(PathBuf, &'a PackageManifest)];

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> IntoIterator for DeclaredManifests<'a> {
    type Item = &'a (PathBuf, &'a PackageManifest);
    type IntoIter = std::slice::Iter<'a, (PathBuf, &'a PackageManifest)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Where the hooks that rewrite the project manifests during an install come
/// from. The check runs no hook, so it cannot see what one rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestHookSource {
    /// The pnpmfiles the configuration finds, if any.
    Pnpmfiles,
    /// Hooks the caller holds in memory in place of the pnpmfiles.
    InMemory,
}

/// How the check reads the project manifests it is handed.
#[derive(Clone, Copy)]
pub struct RepeatInstallManifests<'a> {
    /// The manifests projects expose as injected dependencies of other
    /// importers, keyed by project directory, for the projects whose one
    /// differs from their importer manifest (the Node-API binding's
    /// `dependencyManifest`). The content check compares an injected
    /// project's lockfile snapshot with this manifest. `None` when no
    /// project has one.
    pub dependency_manifests: Option<&'a ProjectManifestsByDir<'a>>,
    /// Every workspace project, when the projects the check is handed are
    /// fewer because each project has its own lockfile. An importer links
    /// the commands of the workspace projects it depends on, so their `bin`
    /// fields are part of what proves it installed. `None` when the projects
    /// the check is handed are the whole workspace.
    pub workspace_projects: Option<&'a [(PathBuf, &'a PackageManifest)]>,
    pub hook_source: ManifestHookSource,
}

impl RepeatInstallManifests<'_> {
    /// The `package.json` files on disk, which carry no dependency manifests,
    /// rewritten by the pnpmfiles.
    pub const ON_DISK: Self = Self {
        dependency_manifests: None,
        workspace_projects: None,
        hook_source: ManifestHookSource::Pnpmfiles,
    };
}
