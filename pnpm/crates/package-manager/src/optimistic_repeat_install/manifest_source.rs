//! How the repeat-install check reads the project manifests it is handed.

use crate::install::ProjectManifestsByDir;

/// How the check learns whether a project manifest may have changed since
/// the previous install validated it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestFreshness {
    /// The manifests are the `package.json` files on disk: one whose mtime
    /// is no newer than the recorded `lastValidatedTimestamp` is unchanged,
    /// and only a newer one is content-checked against the lockfile.
    Mtime,
    /// The manifests were supplied in memory (the Node-API binding). Nothing
    /// on disk records when they changed, and a `package.json` may not even
    /// exist at the project root, so every one is content-checked against
    /// the wanted lockfile.
    Content,
}

/// How the check reads the project manifests it is handed.
#[derive(Clone, Copy)]
pub struct RepeatInstallManifests<'a> {
    pub freshness: ManifestFreshness,
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
    pub const ON_DISK: Self =
        Self { freshness: ManifestFreshness::Mtime, dependency_manifests: None };
}
