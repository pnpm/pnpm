//! The manifest that describes a local directory. The resolver and the
//! lockfile checks both read it through [`local_directory_manifest`].

use pnpm_package_manifest::{
    PackageManifestError,
    publishes_from_directory,
    safe_read_project_manifest_from_dir,
};
use pnpm_resolving_resolver_base::WorkspacePackages;
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        OnceLock,
    },
};

/// What a dependency does with its local directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDirectoryUse {
    /// `file:` copies the directory. The project inside the workspace that
    /// publishes from it describes it, so a `package.json` a build writes
    /// there does not change the resolution.
    Copied,
    /// `link:` links the directory, which keeps its own manifest.
    Linked,
}

/// Which ancestors of a directory without a manifest are searched for the
/// project that publishes from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishingProjectSearch {
    Anywhere,
    InsideWorkspace,
}

/// A local directory and where its manifest may come from.
pub struct LocalDirectory<'a> {
    pub dir: &'a Path,
    pub usage: LocalDirectoryUse,
    /// Bounds the publishing project that describes a
    /// [`LocalDirectoryUse::Copied`] directory.
    pub workspace_root: &'a Path,
    /// The manifest the caller holds in memory for the workspace project at
    /// a lexically normalized directory. It replaces that project's manifest
    /// file.
    pub project_manifest: &'a dyn Fn(&Path) -> Option<&'a Value>,
    pub publishing_project_search: PublishingProjectSearch,
}

/// The manifest that describes `directory`, before any hook, from the first
/// source that has one:
///
/// 1. for a [`LocalDirectoryUse::Copied`] directory, the nearest project
///    inside the workspace whose `publishConfig.directory` it is;
/// 2. the workspace project at the directory;
/// 3. the directory's own manifest file;
/// 4. the nearest project whose `publishConfig.directory` it is, among the
///    ancestors [`LocalDirectory::publishing_project_search`] names.
///
/// `None` when nothing describes it: [`directory_name_manifest`] then stands
/// in for a directory the caller accepts.
///
/// A project manifest that fails to read is no publishing project in step 1
/// and an error in step 4, which runs only for a directory without a
/// manifest.
pub fn local_directory_manifest(
    directory: &LocalDirectory<'_>,
) -> Result<Option<Value>, PackageManifestError> {
    let target = pnpm_fs::lexical_normalize(directory.dir);
    let workspace_root = pnpm_fs::lexical_normalize(directory.workspace_root);
    if directory.usage == LocalDirectoryUse::Copied
        && let Some(manifest) =
            publishing_project_manifest(directory, &target, Some(&workspace_root), |_| Ok(()))?
    {
        return Ok(Some(manifest));
    }
    if let Some(manifest) = (directory.project_manifest)(&target) {
        return Ok(Some(manifest.clone()));
    }
    if let Some(manifest) = safe_read_project_manifest_from_dir(directory.dir)? {
        return Ok(Some(manifest));
    }
    let bound = match directory.publishing_project_search {
        PublishingProjectSearch::Anywhere => None,
        PublishingProjectSearch::InsideWorkspace => Some(workspace_root.as_path()),
    };
    publishing_project_manifest(directory, &target, bound, Err)
}

/// The manifest of a directory that nothing describes: its name alone. The
/// version is unknown, so ranged `packageExtensions` and overrides selectors
/// do not match it (<https://github.com/pnpm/pnpm/issues/15007>).
#[must_use]
pub fn directory_name_manifest(dir: &Path) -> Value {
    let name = dir
        .file_name()
        .map(std::ffi::OsStr::to_string_lossy)
        .unwrap_or_default();
    serde_json::json!({ "name": name })
}

/// The manifest of the nearest ancestor of `target`, inside `bound` when it
/// is given, whose `publishConfig.directory` is `target`. `on_unreadable`
/// decides whether an ancestor manifest that fails to read is skipped or
/// fails the search.
fn publishing_project_manifest(
    directory: &LocalDirectory<'_>,
    target: &Path,
    bound: Option<&Path>,
    on_unreadable: impl Fn(PackageManifestError) -> Result<(), PackageManifestError>,
) -> Result<Option<Value>, PackageManifestError> {
    let candidates = target
        .ancestors()
        .skip(1)
        .take_while(|candidate| bound.is_none_or(|bound| candidate.starts_with(bound)));
    for candidate in candidates {
        let manifest = match (directory.project_manifest)(candidate) {
            Some(manifest) => Cow::Borrowed(manifest),
            None => match safe_read_project_manifest_from_dir(candidate) {
                Ok(Some(manifest)) => Cow::Owned(manifest),
                Ok(None) => continue,
                Err(error) => {
                    on_unreadable(error)?;
                    continue;
                }
            },
        };
        if publishes_from_directory(candidate, &manifest, target) {
            return Ok(Some(manifest.into_owned()));
        }
    }
    Ok(None)
}

/// The workspace projects' manifests by project directory: a view of the
/// [`WorkspacePackages`] the `workspace:` range form resolves from, so a
/// project reached by a path gets the manifest it gets when reached by name.
#[derive(Debug)]
pub struct WorkspaceProjectManifests {
    packages: Arc<WorkspacePackages>,
    /// Each project's name and version in `packages`, by its lexically
    /// normalized directory. Built by the first lookup, since most installs
    /// resolve no local directory.
    keys_by_dir: OnceLock<HashMap<PathBuf, (String, String)>>,
}

impl WorkspaceProjectManifests {
    #[must_use]
    pub fn new(packages: Arc<WorkspacePackages>) -> Self {
        Self { packages, keys_by_dir: OnceLock::new() }
    }

    /// Whether this is a view of `packages`.
    #[must_use]
    pub fn views(&self, packages: &Arc<WorkspacePackages>) -> bool {
        Arc::ptr_eq(&self.packages, packages)
    }

    #[must_use]
    pub fn manifest_at(&self, normalized_dir: &Path) -> Option<&Value> {
        let (name, version) = self.keys_by_dir.get_or_init(|| self.index()).get(normalized_dir)?;
        Some(&self.packages.get(name)?.get(version)?.manifest)
    }

    fn index(&self) -> HashMap<PathBuf, (String, String)> {
        self.packages
            .iter()
            .flat_map(|(name, versions)| {
                versions
                    .iter()
                    .map(move |(version, package)| {
                        let dir = pnpm_fs::lexical_normalize(&package.root_dir);
                        (dir, (name.clone(), version.clone()))
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
