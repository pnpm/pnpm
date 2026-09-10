use crate::{
    State,
    cli_args::patch_state::{EditDirState, StateFileError, read_edit_dir_state},
};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use paths::{
    PatchFileWriteContext, clean_source_dir, cleanup_after_diff, normalize_patches_dir_name,
    path_from_forward_slash, remove_dir_if_exists, write_patch_file_atomically,
};
use pnpm_crypto_hash::create_short_hash;
use pnpm_fs::{is_subdir, lexical_normalize};
use pnpm_lockfile::{LoadLockfileError, Lockfile, PackageKey};
use pnpm_package_manager::{
    PatchCandidate, PatchCandidateSet, PatchTarget, PatchTargetError, PkgFilesForDiff,
    WritePackageForPatch, WritePackageForPatchError, diff_folders, patch_candidates_from_lockfile,
    prepare_pkg_files_for_diff,
};
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
use pnpm_reporter::Reporter;
use pnpm_workspace_manifest_writer::UpdateWorkspaceManifestError;
use serde_json::Value;
use std::{
    fs, io,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct PatchCommitArgs {
    /// Directory created by `pnpm patch`.
    pub patch_dir: PathBuf,
    /// The generated patch file will be saved to this directory.
    #[clap(long = "patches-dir", value_name = "dir")]
    pub patches_dir: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub(crate) enum PatchCommitError {
    #[display("{} is not a valid patch directory", patch_dir.display())]
    #[diagnostic(
        code(ERR_PNPM_INVALID_PATCH_DIR),
        help("A valid patch directory should be created by `pnpm patch`")
    )]
    InvalidPatchDir { patch_dir: PathBuf },

    #[display("Missing package manifest field `{field}` in {}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_COMMIT_MISSING_MANIFEST_FIELD))]
    MissingManifestField { path: PathBuf, field: &'static str },

    #[display("Failed to read package manifest from {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_COMMIT_READ_MANIFEST))]
    ReadManifest {
        path: PathBuf,
        #[error(source)]
        source: PackageManifestError,
    },

    #[display("The modules directory is not ready for patching")]
    #[diagnostic(code(ERR_PNPM_PATCH_NO_LOCKFILE), help("Run `pnpm install` first"))]
    PatchNoLockfile,

    #[display("Failed to create patches directory {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_COMMIT_CREATE_PATCHES_DIR))]
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
    #[diagnostic(code(ERR_PNPM_PATCH_COMMIT_READ_PATCH_FILE_METADATA))]
    ReadPatchFileMetadata {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to write patch file {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_COMMIT_WRITE_PATCH))]
    WritePatch {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to clean up temporary patch directory {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_COMMIT_CLEANUP_TEMP_DIR))]
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
    PatchCommit(#[error(source)] pnpm_package_manager::PatchCommitError),

    #[diagnostic(transparent)]
    UpdateWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),
}

impl PatchCommitArgs {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        state: State,
    ) -> Result<bool, PatchCommitError> {
        let patch_dir = resolve_path(dir, &self.patch_dir);
        let (name, version) = patched_identity(&patch_dir)?;
        let state_value = read_edit_dir_state(&state.config.modules_dir, &patch_dir)
            .map_err(PatchCommitError::StateFile)?
            .ok_or_else(|| PatchCommitError::InvalidPatchDir { patch_dir: patch_dir.clone() })?;

        let current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&state.config.virtual_store_dir)
                .map_err(PatchCommitError::LoadLockfile)?
                .ok_or(PatchCommitError::PatchNoLockfile)?;
        let target = patch_target_from_state(&state_value, &name, &version, &current_lockfile)?;

        let patch_content =
            diff_against_clean::<Reporter>(&state, &patch_dir, &target, &current_lockfile).await?;

        if patch_content.is_empty() {
            println!("No changes were found to the following directory: {}", patch_dir.display());
            return Ok(false);
        }

        self.record_patch(&state, dir, &name, &version, state_value.apply_to_all, &patch_content)?;
        Ok(true)
    }

    /// Write the patch under the patches directory and record it in the
    /// workspace's `patchedDependencies`.
    fn record_patch(
        &self,
        state: &State,
        dir: &Path,
        name: &str,
        version: &str,
        apply_to_all: bool,
        patch_content: &str,
    ) -> Result<(), PatchCommitError> {
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

        let patch_key = if apply_to_all { name.to_string() } else { format!("{name}@{version}") };
        let patch_file_name = format!("{}.patch", patch_key.replace('/', "__"));
        let patch_file_path = patch_file_context.patch_file_path(&patch_file_name)?;
        write_patch_file_atomically(&patch_file_path, patch_content.as_bytes()).map_err(
            |source| PatchCommitError::WritePatch { path: patch_file_path.clone(), source },
        )?;

        let mut patched_dependencies =
            state.config.patched_dependencies.clone().unwrap_or_default();
        patched_dependencies.insert(patch_key, format!("{patches_dir_name}/{patch_file_name}"));
        pnpm_workspace_manifest_writer::set_patched_dependencies(
            &workspace_dir,
            &patched_dependencies,
        )
        .map_err(PatchCommitError::UpdateWorkspaceManifest)
    }
}

/// The patched package's name and version, from the manifest `pnpm patch`
/// left in the directory.
fn patched_identity(patch_dir: &Path) -> Result<(String, String), PatchCommitError> {
    let manifest_path = patch_dir.join("package.json");
    let patched_manifest = PackageManifest::from_path(manifest_path.clone())
        .map_err(|source| PatchCommitError::ReadManifest { path: manifest_path.clone(), source })?;
    Ok((
        manifest_string(patched_manifest.value(), "name", &manifest_path)?,
        manifest_string(patched_manifest.value(), "version", &manifest_path)?,
    ))
}

/// The diff between the package as installed and the edited copy, with
/// the temporary clean copy removed either way.
async fn diff_against_clean<Reporter: self::Reporter + 'static>(
    state: &State,
    patch_dir: &Path,
    target: &PatchTarget,
    current_lockfile: &Lockfile,
) -> Result<String, PatchCommitError> {
    let clean_dir = clean_source_dir(state, patch_dir);
    remove_dir_if_exists(&clean_dir)
        .map_err(|source| PatchCommitError::CleanupTempDir { path: clean_dir.clone(), source })?;
    WritePackageForPatch {
        tarball_mem_cache: &state.tarball_mem_cache,
        http_client: &state.http_client,
        config: state.config,
        current_lockfile,
        target,
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

    let filtered = match prepare_pkg_files_for_diff(patch_dir) {
        Ok(filtered) => filtered,
        Err(source) => {
            remove_dir_if_exists(&clean_dir).map_err(|cleanup_source| {
                PatchCommitError::CleanupTempDir { path: clean_dir.clone(), source: cleanup_source }
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
    Ok(patch_content)
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

#[cfg(test)]
mod tests;

mod paths;
