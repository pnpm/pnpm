//! How the repeat-install check reads the project manifests it is handed.

use crate::install::ProjectManifestsByDir;
use pnpm_package_manifest::PackageManifest;
use std::path::PathBuf;

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
}

impl RepeatInstallManifests<'_> {
    /// The `package.json` files on disk, which carry no dependency manifests.
    pub const ON_DISK: Self = Self { dependency_manifests: None, workspace_projects: None };
}
