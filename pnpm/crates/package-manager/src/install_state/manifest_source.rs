//! How the repeat-install check reads the project manifests it is handed.

use super::ProjectManifestsByDir;
use pnpm_hooks::PnpmfileHooks;
use pnpm_package_manifest::PackageManifest;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The project manifests as the projects declare them: read from disk, or as
/// a mutating command writes them after the install. The workspace state
/// records their digests and the next check compares those with the files,
/// so a view that `packageExtensions` or a `readPackage` hook rewrote must
/// never stand in for them.
#[derive(Clone, Copy)]
pub struct DeclaredManifests<'a> {
    manifests: &'a [(PathBuf, &'a PackageManifest)],
    recorded_digests: Option<&'a HashMap<PathBuf, String>>,
}

impl<'a> DeclaredManifests<'a> {
    #[must_use]
    pub fn new(manifests: &'a [(PathBuf, &'a PackageManifest)]) -> Self {
        Self { manifests, recorded_digests: None }
    }

    /// Manifests some of which the last validation's records stand in for,
    /// per [`crate::install_state::validation::manifest_stand_ins`].
    /// `recorded_digests` holds the digest of each of those, by project
    /// directory. The check then never writes: what it would write is
    /// derived from the files.
    #[must_use]
    pub(crate) fn with_stand_ins(
        manifests: &'a [(PathBuf, &'a PackageManifest)],
        recorded_digests: &'a HashMap<PathBuf, String>,
    ) -> Self {
        Self { manifests, recorded_digests: Some(recorded_digests) }
    }

    #[must_use]
    pub fn as_slice(self) -> &'a [(PathBuf, &'a PackageManifest)] {
        self.manifests
    }

    /// Whether records stand in for some manifests.
    pub(crate) fn has_stand_ins(self) -> bool {
        self.recorded_digests.is_some()
    }

    /// The recorded digest of the manifest of `root_dir`, when a record
    /// stands in for it.
    pub(crate) fn recorded_digest(self, root_dir: &Path) -> Option<&'a str> {
        self.recorded_digests?.get(root_dir).map(String::as_str)
    }
}

impl<'a> std::ops::Deref for DeclaredManifests<'a> {
    type Target = [(PathBuf, &'a PackageManifest)];

    fn deref(&self) -> &Self::Target {
        self.manifests
    }
}

impl<'a> IntoIterator for DeclaredManifests<'a> {
    type Item = &'a (PathBuf, &'a PackageManifest);
    type IntoIter = std::slice::Iter<'a, (PathBuf, &'a PackageManifest)>;

    fn into_iter(self) -> Self::IntoIter {
        self.manifests.iter()
    }
}

/// The hooks that rewrite the project manifests during an install.
#[derive(Clone, Copy)]
pub enum ManifestHooks<'a> {
    /// The pnpmfiles the configuration finds. The check loads them only when
    /// it compares a modified manifest with the lockfile.
    Pnpmfiles,
    /// Hooks the caller holds in place of the pnpmfiles.
    InMemory(&'a Arc<dyn PnpmfileHooks>),
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
    pub hooks: ManifestHooks<'a>,
}

impl RepeatInstallManifests<'_> {
    /// The `package.json` files on disk, which carry no dependency manifests,
    /// rewritten by the pnpmfiles.
    pub const ON_DISK: Self = Self {
        dependency_manifests: None,
        workspace_projects: None,
        hooks: ManifestHooks::Pnpmfiles,
    };
}
