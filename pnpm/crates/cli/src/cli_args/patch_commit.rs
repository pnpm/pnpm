use crate::{
    State,
    cli_args::patch_state::{EditDirState, StateFileError, read_edit_dir_state},
};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_crypto_hash::create_short_hash;
use pacquet_fs::{is_subdir, lexical_normalize};
use pacquet_lockfile::{LoadLockfileError, Lockfile, PackageKey};
use pacquet_package_manager::{
    PatchCandidate, PatchCandidateSet, PatchTarget, PatchTargetError, PkgFilesForDiff,
    WritePackageForPatch, WritePackageForPatchError, diff_folders, patch_candidates_from_lockfile,
    prepare_pkg_files_for_diff,
};
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_reporter::Reporter;
use pacquet_workspace_manifest_writer::UpdateWorkspaceManifestError;
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct PatchCommitArgs {
    /// Directory created by `pacquet patch`.
    pub patch_dir: PathBuf,
    /// The generated patch file will be saved to this directory.
    #[clap(long = "patches-dir", value_name = "dir")]
    pub patches_dir: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PatchCommitError {
    #[display("{} is not a valid patch directory", patch_dir.display())]
    #[diagnostic(
        code(ERR_PNPM_INVALID_PATCH_DIR),
        help("A valid patch directory should be created by `pacquet patch`")
    )]
    InvalidPatchDir { patch_dir: PathBuf },

    #[display("Missing package manifest field `{field}` in {}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_missing_manifest_field))]
    MissingManifestField { path: PathBuf, field: &'static str },

    #[display("Failed to read package manifest from {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_read_manifest))]
    ReadManifest {
        path: PathBuf,
        #[error(source)]
        source: PackageManifestError,
    },

    #[display("The modules directory is not ready for patching")]
    #[diagnostic(code(ERR_PNPM_PATCH_NO_LOCKFILE), help("Run pacquet install first"))]
    PatchNoLockfile,

    #[display("Failed to create patches directory {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_create_patches_dir))]
    CreatePatchesDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("The configured patches directory is outside the project: {patches_dir}")]
    #[diagnostic(code(ERR_PNPM_PATCHES_DIR_OUTSIDE_PROJECT))]
    PatchesDirOutsideProject { patches_dir: String },

    #[display("Patch file \"{patch_file}\" is outside the configured patches directory")]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR))]
    PatchFileOutsidePatchesDir { patch_file: String },

    #[display("Failed to read patch file metadata for {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_read_patch_file_metadata))]
    ReadPatchFileMetadata {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to write patch file {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_write_patch))]
    WritePatch {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to clean up temporary patch directory {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_cleanup_temp_dir))]
    CleanupTempDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[diagnostic(transparent)]
    StateFile(#[error(source)] StateFileError),

    #[diagnostic(transparent)]
    LoadLockfile(#[error(source)] LoadLockfileError),

    #[diagnostic(transparent)]
    PatchTarget(#[error(source)] PatchTargetError),

    #[diagnostic(transparent)]
    WritePackage(#[error(source)] WritePackageForPatchError),

    #[diagnostic(transparent)]
    PatchCommit(#[error(source)] pacquet_package_manager::PatchCommitError),

    #[diagnostic(transparent)]
    UpdateWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),
}

impl PatchCommitArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        state: State,
    ) -> Result<bool, PatchCommitError> {
        let patch_dir = resolve_path(dir, &self.patch_dir);
        let manifest_path = patch_dir.join("package.json");
        let patched_manifest =
            PackageManifest::from_path(manifest_path.clone()).map_err(|source| {
                PatchCommitError::ReadManifest { path: manifest_path.clone(), source }
            })?;
        let name = manifest_string(patched_manifest.value(), "name", &manifest_path)?;
        let version = manifest_string(patched_manifest.value(), "version", &manifest_path)?;
        let state_value = read_edit_dir_state(&state.config.modules_dir, &patch_dir)
            .map_err(PatchCommitError::StateFile)?
            .ok_or_else(|| PatchCommitError::InvalidPatchDir { patch_dir: patch_dir.clone() })?;

        let current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&state.config.virtual_store_dir)
                .map_err(PatchCommitError::LoadLockfile)?
                .ok_or(PatchCommitError::PatchNoLockfile)?;
        let target = patch_target_from_state(&state_value, &name, &version, &current_lockfile)?;

        let clean_dir = clean_source_dir(&state, &patch_dir);
        remove_dir_if_exists(&clean_dir).map_err(|source| PatchCommitError::CleanupTempDir {
            path: clean_dir.clone(),
            source,
        })?;
        WritePackageForPatch {
            tarball_mem_cache: &state.tarball_mem_cache,
            http_client: &state.http_client,
            config: state.config,
            current_lockfile: &current_lockfile,
            target: &target,
            dest: &clean_dir,
        }
        .run::<Reporter>()
        .await
        .map_err(|source| match remove_dir_if_exists(&clean_dir) {
            Ok(()) => PatchCommitError::WritePackage(source),
            Err(cleanup_source) => {
                PatchCommitError::CleanupTempDir { path: clean_dir.clone(), source: cleanup_source }
            }
        })?;

        let filtered = match prepare_pkg_files_for_diff(&patch_dir) {
            Ok(filtered) => filtered,
            Err(source) => {
                remove_dir_if_exists(&clean_dir).map_err(|cleanup_source| {
                    PatchCommitError::CleanupTempDir {
                        path: clean_dir.clone(),
                        source: cleanup_source,
                    }
                })?;
                return Err(PatchCommitError::PatchCommit(source));
            }
        };
        let filtered_path = match &filtered {
            PkgFilesForDiff::Original(path) | PkgFilesForDiff::Temporary(path) => path,
        };
        let patch_content = match diff_folders(&clean_dir, filtered_path) {
            Ok(patch_content) => patch_content,
            Err(source) => {
                cleanup_after_diff(&clean_dir, &filtered)?;
                return Err(PatchCommitError::PatchCommit(source));
            }
        };
        cleanup_after_diff(&clean_dir, &filtered)?;

        if patch_content.is_empty() {
            println!("No changes were found to the following directory: {}", patch_dir.display());
            return Ok(false);
        }

        let workspace_dir = state.config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let patches_dir_name = normalize_patches_dir_name(
            self.patches_dir
                .as_deref()
                .or(state.config.patches_dir.as_deref())
                .unwrap_or("patches"),
        );
        let patches_dir = workspace_dir.join(path_from_forward_slash(&patches_dir_name));
        fs::create_dir_all(&patches_dir).map_err(|source| PatchCommitError::CreatePatchesDir {
            path: patches_dir.clone(),
            source,
        })?;
        let patch_file_context = PatchFileWriteContext::new(&workspace_dir, &patches_dir_name)?;

        let patch_key =
            if state_value.apply_to_all { name.clone() } else { format!("{name}@{version}") };
        let patch_file_name = format!("{}.patch", patch_key.replace('/', "__"));
        let patch_file_path = patch_file_context.patch_file_path(&patch_file_name)?;
        write_patch_file_atomically(&patch_file_path, patch_content.as_bytes()).map_err(
            |source| PatchCommitError::WritePatch { path: patch_file_path.clone(), source },
        )?;

        let mut patched_dependencies =
            state.config.patched_dependencies.clone().unwrap_or_default();
        patched_dependencies.insert(patch_key, format!("{patches_dir_name}/{patch_file_name}"));
        pacquet_workspace_manifest_writer::set_patched_dependencies(
            &workspace_dir,
            &patched_dependencies,
        )
        .map_err(PatchCommitError::UpdateWorkspaceManifest)?;

        Ok(true)
    }
}

fn manifest_string(
    manifest: &Value,
    field: &'static str,
    path: &Path,
) -> Result<String, PatchCommitError> {
    manifest
        .get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PatchCommitError::MissingManifestField { path: path.to_path_buf(), field })
}

fn patch_target_from_state(
    state_value: &EditDirState,
    name: &str,
    version: &str,
    current_lockfile: &Lockfile,
) -> Result<PatchTarget, PatchCommitError> {
    let fallback = format!("{name}@{version}");
    let set = patch_candidates_from_lockfile(&state_value.patched_pkg, current_lockfile)
        .or_else(|_| patch_candidates_from_lockfile(&fallback, current_lockfile))
        .map_err(PatchCommitError::PatchTarget)?;
    let candidate = matching_candidate(&set, version, state_value.package_key.as_ref())
        .ok_or_else(|| {
            PatchCommitError::PatchTarget(PatchTargetError::VersionNotFound {
                requested: fallback.clone(),
                hint: format!("did you forget to install {fallback}?"),
            })
        })?;
    Ok(PatchTarget {
        alias: name.to_string(),
        version: version.to_string(),
        bare_specifier: candidate.git_tarball_url.clone().unwrap_or_else(|| version.to_string()),
        apply_to_all: state_value.apply_to_all,
        git_tarball_url: candidate.git_tarball_url.clone(),
        package_key: candidate.package_key,
    })
}

fn matching_candidate(
    set: &PatchCandidateSet,
    version: &str,
    package_key: Option<&PackageKey>,
) -> Option<PatchCandidate> {
    set.preferred_versions
        .iter()
        .chain(set.versions.iter())
        .find(|candidate| {
            candidate.version == version
                && package_key.is_none_or(|package_key| candidate.package_key == *package_key)
        })
        .cloned()
}

fn resolve_path(dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { dir.join(path) }
}

struct PatchFileWriteContext {
    patches_dir: PathBuf,
    real_patches_dir: PathBuf,
}

impl PatchFileWriteContext {
    fn new(lockfile_dir: &Path, patches_dir_setting: &str) -> Result<Self, PatchCommitError> {
        let project_root = lexical_normalize(lockfile_dir);
        let real_project_root =
            dunce::canonicalize(&project_root).unwrap_or_else(|_| project_root.clone());
        let patches_dir = join_setting_path(&project_root, patches_dir_setting);
        if !is_subdir(&project_root, &patches_dir) {
            return Err(PatchCommitError::PatchesDirOutsideProject {
                patches_dir: patches_dir_setting.to_string(),
            });
        }
        let real_patches_dir = dunce::canonicalize(&patches_dir).map_err(|source| {
            PatchCommitError::ReadPatchFileMetadata { path: patches_dir.clone(), source }
        })?;
        if !is_subdir(&real_project_root, &real_patches_dir) {
            return Err(PatchCommitError::PatchesDirOutsideProject {
                patches_dir: patches_dir_setting.to_string(),
            });
        }
        Ok(Self { patches_dir, real_patches_dir })
    }

    fn patch_file_path(&self, patch_file: &str) -> Result<PathBuf, PatchCommitError> {
        let target_path = resolve_patch_path(&self.patches_dir, Path::new(patch_file));
        if target_path == self.patches_dir || !is_subdir(&self.patches_dir, &target_path) {
            return Err(PatchCommitError::PatchFileOutsidePatchesDir {
                patch_file: patch_file.to_string(),
            });
        }

        let parent_dir = target_path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let real_parent_dir = dunce::canonicalize(&parent_dir).map_err(|source| {
            PatchCommitError::ReadPatchFileMetadata { path: parent_dir.clone(), source }
        })?;
        if !is_subdir(&self.real_patches_dir, &real_parent_dir) {
            return Err(PatchCommitError::PatchFileOutsidePatchesDir {
                patch_file: patch_file.to_string(),
            });
        }

        if lstat_if_exists(&target_path)?.as_ref().is_some_and(|stats| {
            stats.file_type().is_symlink()
                && dunce::canonicalize(&target_path)
                    .ok()
                    .is_none_or(|real_target| !is_subdir(&self.real_patches_dir, &real_target))
        }) {
            return Err(PatchCommitError::PatchFileOutsidePatchesDir {
                patch_file: patch_file.to_string(),
            });
        }

        Ok(target_path)
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

fn resolve_patch_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { lexical_normalize(path) } else { lexical_normalize(&base.join(path)) }
}

fn lstat_if_exists(path: &Path) -> Result<Option<fs::Metadata>, PatchCommitError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => {
            Err(PatchCommitError::ReadPatchFileMetadata { path: path.to_path_buf(), source })
        }
    }
}

fn write_patch_file_atomically(target: &Path, content: &[u8]) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
    tmp.persist(target).map_err(|error| error.error)?;
    Ok(())
}

fn clean_source_dir(state: &State, patch_dir: &Path) -> PathBuf {
    let hash = create_short_hash(&patch_dir.to_string_lossy());
    state.config.store_dir.tmp().join("patch-commit").join(hash)
}

fn cleanup_after_diff(
    clean_dir: &Path,
    filtered: &PkgFilesForDiff,
) -> Result<(), PatchCommitError> {
    remove_dir_if_exists(clean_dir).map_err(|source| PatchCommitError::CleanupTempDir {
        path: clean_dir.to_path_buf(),
        source,
    })?;
    if let PkgFilesForDiff::Temporary(path) = filtered {
        remove_dir_if_exists(path)
            .map_err(|source| PatchCommitError::CleanupTempDir { path: path.clone(), source })?;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "temporary directory must not be a symbolic link",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn normalize_patches_dir_name(input: &str) -> String {
    let mut parts = Vec::new();
    for component in Path::new(input).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

fn path_from_forward_slash(path: &str) -> PathBuf {
    path.split('/').collect()
}

#[cfg(test)]
mod tests;
