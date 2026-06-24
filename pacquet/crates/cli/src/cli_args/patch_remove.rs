use crate::State;
use clap::Args;
use derive_more::{Display, Error};
use dialoguer::MultiSelect;
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_fs::{is_subdir, lexical_normalize};
use pacquet_workspace_manifest_writer::UpdateWorkspaceManifestError;
use std::{
    collections::HashSet,
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct PatchRemoveArgs {
    /// Patches to remove from patchedDependencies.
    #[clap(value_name = "pkg")]
    pub patches: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PatchRemoveError {
    #[display("There are no patches that need to be removed")]
    #[diagnostic(code(ERR_PNPM_NO_PATCHES_TO_REMOVE))]
    NoPatchesToRemove,

    #[display("Canceled")]
    #[diagnostic(code(ERR_PNPM_PATCH_REMOVE_CANCELED))]
    Canceled,

    #[display("Patch \"{patch}\" not found in patched dependencies")]
    #[diagnostic(code(ERR_PNPM_PATCH_NOT_FOUND))]
    PatchNotFound { patch: String },

    #[display("The configured patches directory is outside the project: {patches_dir}")]
    #[diagnostic(code(ERR_PNPM_PATCHES_DIR_OUTSIDE_PROJECT))]
    PatchesDirOutsideProject { patches_dir: String },

    #[display("Patch file \"{patch_file}\" is outside the configured patches directory")]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR))]
    PatchFileOutsidePatchesDir { patch_file: String },

    #[display("Patch file \"{patch_file}\" is a directory")]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_IS_DIRECTORY))]
    PatchFileIsDirectory { patch_file: String },

    #[display("Failed to remove patch file {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_remove_unlink_patch_file))]
    RemovePatchFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to read patch directory {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_remove_read_patch_dir))]
    ReadPatchDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to remove empty patch directory {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_remove_remove_patch_dir))]
    RemovePatchDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[diagnostic(transparent)]
    UpdateWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),
}

impl PatchRemoveArgs {
    pub async fn run(self, dir: &Path, state: State) -> Result<bool, PatchRemoveError> {
        let mut patched_dependencies =
            state.config.patched_dependencies.clone().unwrap_or_default();
        let patches_to_remove =
            patches_to_remove(self.patches, &patched_dependencies, &DialoguerPatchRemovePrompt)?;
        for patch in &patches_to_remove {
            if !patched_dependencies.contains_key(patch) {
                return Err(PatchRemoveError::PatchNotFound { patch: patch.clone() });
            }
        }

        let lockfile_dir = state.config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let ctx = PatchRemovalContext::new(
            &lockfile_dir,
            state.config.patches_dir.as_deref().unwrap_or("patches"),
        )?;
        let targets = patches_to_remove
            .iter()
            .map(|patch| {
                let patch_file = patched_dependencies
                    .get(patch)
                    .ok_or_else(|| PatchRemoveError::PatchNotFound { patch: patch.clone() })?;
                PatchRemovalTarget::new(patch, patch_file, &ctx)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let removed_patches: HashSet<&String> = patches_to_remove.iter().collect();
        let remaining_patch_files = patched_dependencies
            .iter()
            .filter(|(patch, _)| !removed_patches.contains(patch))
            .map(|(patch, patch_file)| {
                PatchRemovalTarget::new(patch, patch_file, &ctx).map(|target| target.target_path)
            })
            .collect::<Result<HashSet<_>, _>>()?;

        for target in &targets {
            if !remaining_patch_files.contains(&target.target_path) {
                unlink_patch_if_exists(target)?;
            }
        }
        for target in &targets {
            patched_dependencies.shift_remove(&target.patch);
        }
        remove_empty_patch_dirs(&targets)?;

        pacquet_workspace_manifest_writer::set_patched_dependencies(
            &lockfile_dir,
            &patched_dependencies,
        )
        .map_err(PatchRemoveError::UpdateWorkspaceManifest)?;

        Ok(true)
    }
}

fn patches_to_remove(
    patches: Vec<String>,
    patched_dependencies: &IndexMap<String, String>,
    prompt: &impl PatchRemovePrompt,
) -> Result<Vec<String>, PatchRemoveError> {
    if !patches.is_empty() {
        return Ok(patches);
    }
    if patched_dependencies.is_empty() {
        return Err(PatchRemoveError::NoPatchesToRemove);
    }
    let all_patches: Vec<String> = patched_dependencies.keys().cloned().collect();
    prompt.select_patches(&all_patches).and_then(|selected| {
        if selected.is_empty() { Err(PatchRemoveError::NoPatchesToRemove) } else { Ok(selected) }
    })
}

trait PatchRemovePrompt {
    fn select_patches(&self, patches: &[String]) -> Result<Vec<String>, PatchRemoveError>;
}

struct DialoguerPatchRemovePrompt;

impl PatchRemovePrompt for DialoguerPatchRemovePrompt {
    fn select_patches(&self, patches: &[String]) -> Result<Vec<String>, PatchRemoveError> {
        select_patches_from_indices(patches, dialoguer_select_patch_indices)
    }
}

fn dialoguer_select_patch_indices(patches: &[String]) -> Result<Vec<usize>, PatchRemoveError> {
    MultiSelect::new()
        .with_prompt("Select the patch to be removed")
        .items(patches)
        .interact()
        .map_err(|_| PatchRemoveError::Canceled)
}

fn select_patches_from_indices(
    patches: &[String],
    select_indices: impl FnOnce(&[String]) -> Result<Vec<usize>, PatchRemoveError>,
) -> Result<Vec<String>, PatchRemoveError> {
    select_indices(patches)
        .map(|selected_indices| patches_from_selected_indices(patches, selected_indices))
}

fn patches_from_selected_indices(patches: &[String], selected_indices: Vec<usize>) -> Vec<String> {
    selected_indices.into_iter().map(|index| patches[index].clone()).collect()
}

struct PatchRemovalContext {
    project_root: PathBuf,
    patches_dir: PathBuf,
    real_patches_dir: Option<PathBuf>,
}

impl PatchRemovalContext {
    fn new(lockfile_dir: &Path, patches_dir_setting: &str) -> Result<Self, PatchRemoveError> {
        let project_root = lexical_normalize(lockfile_dir);
        let real_project_root =
            dunce::canonicalize(&project_root).unwrap_or_else(|_| project_root.clone());
        let patches_dir = join_setting_path(&project_root, patches_dir_setting);
        if !is_subdir(&project_root, &patches_dir) {
            return Err(PatchRemoveError::PatchesDirOutsideProject {
                patches_dir: patches_dir_setting.to_string(),
            });
        }
        let real_patches_dir = realpath_if_exists(&patches_dir);
        if real_patches_dir.as_ref().is_some_and(|real| !is_subdir(&real_project_root, real)) {
            return Err(PatchRemoveError::PatchesDirOutsideProject {
                patches_dir: patches_dir_setting.to_string(),
            });
        }
        Ok(Self { project_root, patches_dir, real_patches_dir })
    }
}

struct PatchRemovalTarget {
    patch: String,
    parent_dir: PathBuf,
    target_path: PathBuf,
    target_exists: bool,
}

impl PatchRemovalTarget {
    fn new(
        patch: &str,
        patch_file: &str,
        ctx: &PatchRemovalContext,
    ) -> Result<Self, PatchRemoveError> {
        let target_path = resolve_path(&ctx.project_root, Path::new(patch_file));
        if target_path == ctx.patches_dir || !is_subdir(&ctx.patches_dir, &target_path) {
            return Err(PatchRemoveError::PatchFileOutsidePatchesDir {
                patch_file: patch_file.to_string(),
            });
        }

        let parent_dir = target_path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let target_stats = lstat_if_exists(&target_path)?;
        let real_parent_dir = realpath_if_exists(&parent_dir);
        let real_patches_dir =
            ctx.real_patches_dir.clone().or_else(|| realpath_if_exists(&ctx.patches_dir));
        if let (Some(real_parent_dir), Some(real_patches_dir)) = (real_parent_dir, real_patches_dir)
            && !is_subdir(&real_patches_dir, &real_parent_dir)
        {
            return Err(PatchRemoveError::PatchFileOutsidePatchesDir {
                patch_file: patch_file.to_string(),
            });
        }
        if target_stats.as_ref().is_some_and(fs::Metadata::is_dir) {
            return Err(PatchRemoveError::PatchFileIsDirectory {
                patch_file: patch_file.to_string(),
            });
        }

        Ok(Self {
            patch: patch.to_string(),
            parent_dir,
            target_path,
            target_exists: target_stats.is_some(),
        })
    }
}

fn join_setting_path(base: &Path, setting: &str) -> PathBuf {
    let mut joined = base.to_path_buf();
    for component in Path::new(setting).components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::CurDir => {}
            Component::ParentDir => joined.push(".."),
            Component::Normal(part) => joined.push(part),
        }
    }
    lexical_normalize(&joined)
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { lexical_normalize(path) } else { lexical_normalize(&base.join(path)) }
}

fn lstat_if_exists(path: &Path) -> Result<Option<fs::Metadata>, PatchRemoveError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PatchRemoveError::ReadPatchDir {
            path: path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
            source,
        }),
    }
}

fn realpath_if_exists(path: &Path) -> Option<PathBuf> {
    dunce::canonicalize(path).ok()
}

fn unlink_patch_if_exists(target: &PatchRemovalTarget) -> Result<(), PatchRemoveError> {
    if !target.target_exists {
        return Ok(());
    }
    match fs::remove_file(&target.target_path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => {
            Err(PatchRemoveError::RemovePatchFile { path: target.target_path.clone(), source })
        }
    }
}

fn remove_empty_patch_dirs(targets: &[PatchRemovalTarget]) -> Result<(), PatchRemoveError> {
    remove_empty_patch_dirs_with_fs(targets, &RealPatchRemoveFs)
}

fn remove_empty_patch_dirs_with_fs(
    targets: &[PatchRemovalTarget],
    fs_ops: &impl PatchRemoveFs,
) -> Result<(), PatchRemoveError> {
    for target in targets {
        match fs_ops.is_dir_empty(&target.parent_dir) {
            Ok(true) => {
                if let Err(source) = fs_ops.remove_dir(&target.parent_dir) {
                    return Err(PatchRemoveError::RemovePatchDir {
                        path: target.parent_dir.clone(),
                        source,
                    });
                }
            }
            Ok(false) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(PatchRemoveError::ReadPatchDir {
                    path: target.parent_dir.clone(),
                    source,
                });
            }
        }
    }
    Ok(())
}

trait PatchRemoveFs {
    fn is_dir_empty(&self, path: &Path) -> io::Result<bool>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
}

struct RealPatchRemoveFs;

impl PatchRemoveFs for RealPatchRemoveFs {
    fn is_dir_empty(&self, path: &Path) -> io::Result<bool> {
        fs::read_dir(path).map(|mut entries| entries.next().is_none())
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }
}

#[cfg(test)]
mod tests;
