//! How the repeat-install check reads the project manifests it is handed.

use crate::install::ProjectManifestsByDir;

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
}

impl RepeatInstallManifests<'_> {
    /// The `package.json` files on disk, which carry no dependency manifests.
    pub const ON_DISK: Self = Self { dependency_manifests: None };
}
