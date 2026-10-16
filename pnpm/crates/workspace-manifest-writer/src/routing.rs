//! Which file a write to a workspace setting goes to, by the ownership the
//! root manifest's include index declares.

use super::{
    Manifest, UpdateWorkspaceManifestError, UpdateWorkspaceManifestOptions,
    WORKSPACE_MANIFEST_FILENAME, edit_manifest_file, read_if_present, read_manifest_at,
    write_atomic, write_or_remove_manifest,
};
use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pnpm_workspace::{
    INCLUDE_KEY, IncludeError, IncludeSource, SettingOwner, SettingSlot, WorkspaceIncludes,
    canonical_setting_name, rebase_path, rebase_setting, rebase_specifier,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// A file a workspace setting can be written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestTarget {
    pub(crate) path: PathBuf,
    /// A file the root manifest includes, which a write never deletes.
    pub(crate) included: bool,
    /// The directory relative paths in this file are measured from: the
    /// directory of its path as `include` writes it.
    pub(crate) base_dir: PathBuf,
}

impl ManifestTarget {
    /// A root manifest, which an edit that empties it removes.
    pub(crate) fn root_manifest(path: &Path) -> Self {
        ManifestTarget {
            path: path.to_path_buf(),
            included: false,
            base_dir: path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        }
    }

    /// `value`, a path measured from `workspace_dir`, measured from this
    /// file's [`Self::base_dir`].
    pub(crate) fn path_from(&self, workspace_dir: &Path, value: &str) -> String {
        rebase_path(value, workspace_dir, &self.base_dir).unwrap_or_else(|| value.to_string())
    }

    /// The value of the setting `key`, with the relative paths it holds
    /// measured from `workspace_dir`, measured from this file instead.
    pub(crate) fn setting_from(
        &self,
        workspace_dir: &Path,
        key: &str,
        value: &serde_json::Value,
    ) -> serde_json::Value {
        let Ok(mut yaml) = yaml_serde::to_value(value) else {
            return value.clone();
        };
        rebase_setting(canonical_setting_name(key), &mut yaml, workspace_dir, &self.base_dir);
        serde_json::to_value(yaml).unwrap_or_else(|_| value.clone())
    }

    /// [`Self::path_from`] for a dependency specifier, which moves only when
    /// it names a local path.
    pub(crate) fn specifier_from(&self, workspace_dir: &Path, value: &str) -> String {
        rebase_specifier(value, workspace_dir, &self.base_dir).unwrap_or_else(|| value.to_string())
    }
}

/// The workspace manifest of a directory and the files it includes.
pub(crate) struct WorkspaceManifests {
    root: PathBuf,
    includes: Option<WorkspaceIncludes>,
    /// The merged settings, when the root manifest has an include index.
    pub(crate) merged_text: Option<String>,
}

impl WorkspaceManifests {
    /// Read `<dir>/pnpm-workspace.yaml`'s include index. A missing manifest
    /// has none.
    pub(crate) fn read(dir: &Path) -> Result<Self, UpdateWorkspaceManifestError> {
        let root = dir.join(WORKSPACE_MANIFEST_FILENAME);
        let document = pnpm_workspace::read_workspace_manifest_document(dir)
            .map_err(|error| match error {
                IncludeError::ReadRoot { path, source } => {
                    UpdateWorkspaceManifestError::Read { path, source }
                }
                error => UpdateWorkspaceManifestError::Include(error),
            })?;
        let Some(document) = document.filter(|document| document.includes.is_some()) else {
            return Ok(WorkspaceManifests { root, includes: None, merged_text: None });
        };
        Ok(WorkspaceManifests {
            root,
            includes: document.includes,
            merged_text: Some(document.text),
        })
    }

    pub(crate) fn has_includes(&self) -> bool {
        self.includes.is_some()
    }

    /// The file that owns the top-level setting `key`.
    pub(crate) fn target_for_setting(
        &self,
        key: &str,
    ) -> Result<ManifestTarget, UpdateWorkspaceManifestError> {
        let slot = match key {
            INCLUDE_KEY => return Err(UpdateWorkspaceManifestError::IncludeNotWritable),
            "catalog" => SettingSlot::Catalog(DEFAULT_CATALOG_NAME.to_string()),
            "catalogs" => return self.target_for_all_catalogs(),
            _ => SettingSlot::Setting(canonical_setting_name(key).to_string()),
        };
        self.target_for(&slot)
    }

    /// The one file that owns every catalog, for a write of `catalogs` as a
    /// whole.
    fn target_for_all_catalogs(&self) -> Result<ManifestTarget, UpdateWorkspaceManifestError> {
        let target = self.target_for(&SettingSlot::AllCatalogs)?;
        let catalog_slots = self.includes
            .iter()
            .flat_map(|includes| {
                includes.ownership
                    .slots()
                    .filter(|slot| matches!(slot, SettingSlot::Catalog(_)))
            });
        for slot in catalog_slots {
            if self.target_for(slot)? != target {
                return Err(UpdateWorkspaceManifestError::CatalogsSplitAcrossFiles);
            }
        }
        Ok(target)
    }

    /// The file that owns `slot`. A setting no file owns goes to the root
    /// manifest; one a config dependency owns cannot be written.
    pub(crate) fn target_for(
        &self,
        slot: &SettingSlot,
    ) -> Result<ManifestTarget, UpdateWorkspaceManifestError> {
        let owner = self.includes
            .as_ref()
            .map_or(SettingOwner::Unowned, |includes| includes.owner(slot));
        match owner {
            SettingOwner::Unowned | SettingOwner::Root => Ok(self.root_target()),
            SettingOwner::Include(IncludeSource::Local { path, .. }) => Ok(included_target(path)),
            SettingOwner::Include(IncludeSource::Package(name)) => {
                Err(UpdateWorkspaceManifestError::OwnedByConfigDependency {
                    setting: slot.to_string(),
                    name: name.clone(),
                })
            }
        }
    }

    pub(crate) fn root_target(&self) -> ManifestTarget {
        ManifestTarget::root_manifest(&self.root)
    }

    pub(crate) fn workspace_dir(&self) -> PathBuf {
        self.root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// The root manifest and every local file it includes.
    pub(crate) fn targets(&self) -> Vec<ManifestTarget> {
        let included = self.includes
            .iter()
            .flat_map(WorkspaceIncludes::local_paths)
            .map(included_target);
        std::iter::once(self.root_target()).chain(included).collect()
    }
}

/// An included file, written through a symlink, which the root manifest names
/// on purpose, rather than replacing it.
fn included_target(path: &Path) -> ManifestTarget {
    ManifestTarget {
        path: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        included: true,
        base_dir: path
            .parent()
            .map(pnpm_fs::lexical_normalize)
            .unwrap_or_default(),
    }
}

/// The file of `dir`'s workspace that owns the setting `key`, parsed. A file
/// that does not exist yet parses as an empty document.
pub(crate) fn read_owning_manifest(
    dir: &Path,
    key: &str,
) -> Result<(ManifestTarget, Manifest), UpdateWorkspaceManifestError> {
    let target = WorkspaceManifests::read(dir)?.target_for_setting(key)?;
    let manifest = read_manifest_at(&target.path)?;
    Ok((target, manifest))
}

/// [`crate::update_workspace_manifest`] for settings spread over several
/// files: each catalog entry and exclude goes to the file that owns it, and
/// the cleanup passes run in every file, which holds only what it owns.
pub(crate) fn update_included_manifests(
    manifests: &WorkspaceManifests,
    opts: &UpdateWorkspaceManifestOptions<'_>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let merged = manifests.merged_text
        .as_deref()
        .map(|text| Manifest::parse(Some(text)))
        .transpose()
        .map_err(|source| UpdateWorkspaceManifestError::Parse {
            path: manifests.root.clone(),
            source,
        })?;
    let catalogs = route_catalogs(manifests, opts.updated_catalogs)?;
    let excludes_target = (!opts.added_minimum_release_age_excludes.is_empty())
        .then(|| manifests.target_for_setting("minimumReleaseAgeExclude"))
        .transpose()?;
    let mut edited = Vec::new();
    for target in manifests.targets() {
        let excludes_here = excludes_target.as_ref() == Some(&target);
        let file_opts = UpdateWorkspaceManifestOptions {
            updated_catalogs: catalogs.get(&target.path),
            added_minimum_release_age_excludes: if excludes_here {
                opts.added_minimum_release_age_excludes
            } else {
                &[]
            },
            ..*opts
        };
        if let Some(manifest) = edit_manifest_file(&target, &file_opts, merged.as_ref())? {
            edited.push((target, manifest));
        }
    }
    write_all_or_restore(edited)
}

/// Write every edited file. When a write fails, the files written before it
/// get their original content back.
fn write_all_or_restore(
    edited: Vec<(ManifestTarget, Manifest)>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let originals = edited
        .iter()
        .map(|(target, _)| read_if_present(&target.path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut written: Vec<(PathBuf, Option<String>)> = Vec::with_capacity(edited.len());
    for ((target, manifest), original) in edited.into_iter().zip(originals) {
        if let Err(error) = write_or_remove_manifest(&target, manifest) {
            for (path, original) in written {
                restore(&path, original);
            }
            return Err(error);
        }
        written.push((target.path, original));
    }
    Ok(())
}

fn restore(path: &Path, original: Option<String>) {
    let _ = match original {
        Some(text) => write_atomic(path, &text),
        None => fs::remove_file(path),
    };
}

/// The updated catalog entries, grouped by the file that owns each catalog.
fn route_catalogs(
    manifests: &WorkspaceManifests,
    updated_catalogs: Option<&Catalogs>,
) -> Result<HashMap<PathBuf, Catalogs>, UpdateWorkspaceManifestError> {
    let workspace_dir = manifests.workspace_dir();
    let mut routed: HashMap<PathBuf, Catalogs> = HashMap::new();
    for (name, entries) in updated_catalogs.into_iter().flatten() {
        if entries.is_empty() {
            continue;
        }
        let target = manifests.target_for(&SettingSlot::Catalog(name.clone()))?;
        let entries = entries
            .iter()
            .map(|(alias, specifier)| {
                (alias.clone(), target.specifier_from(&workspace_dir, specifier))
            })
            .collect();
        routed
            .entry(target.path)
            .or_default()
            .insert(name.clone(), entries);
    }
    Ok(routed)
}
