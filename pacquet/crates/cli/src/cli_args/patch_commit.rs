use crate::{
    State,
    cli_args::patch_state::{StateFileError, read_edit_dir_state},
};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_crypto_hash::create_short_hash;
use pacquet_lockfile::{LoadLockfileError, Lockfile};
use pacquet_package_manager::{
    PatchTarget, PatchTargetError, PkgFilesForDiff, WritePackageForPatch,
    WritePackageForPatchError, diff_folders, patch_candidates_from_lockfile,
    prepare_pkg_files_for_diff,
};
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_reporter::Reporter;
use pacquet_workspace_manifest_writer::UpdateWorkspaceManifestError;
use serde_json::Value;
use std::{
    fs, io,
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

    #[display("Failed to write patch file {}: {source}", path.display())]
    #[diagnostic(code(pacquet::patch_commit_write_patch))]
    WritePatch {
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
        let target = patch_target_from_state(
            &state_value.patched_pkg,
            &name,
            &version,
            state_value.apply_to_all,
            &current_lockfile,
        )?;

        let clean_dir = clean_source_dir(&state, &patch_dir);
        remove_dir_if_exists(&clean_dir);
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
        .map_err(PatchCommitError::WritePackage)?;

        let filtered =
            prepare_pkg_files_for_diff(&patch_dir).map_err(PatchCommitError::PatchCommit)?;
        let filtered_path = match &filtered {
            PkgFilesForDiff::Original(path) | PkgFilesForDiff::Temporary(path) => path,
        };
        let patch_content =
            diff_folders(&clean_dir, filtered_path).map_err(PatchCommitError::PatchCommit)?;
        cleanup_after_diff(&clean_dir, &filtered);

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

        let patch_key =
            if state_value.apply_to_all { name.clone() } else { format!("{name}@{version}") };
        let patch_file_name = format!("{}.patch", patch_key.replace('/', "__"));
        let patch_file_path = patches_dir.join(&patch_file_name);
        fs::write(&patch_file_path, patch_content).map_err(|source| {
            PatchCommitError::WritePatch { path: patch_file_path.clone(), source }
        })?;

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
    raw_dependency: &str,
    name: &str,
    version: &str,
    apply_to_all: bool,
    current_lockfile: &Lockfile,
) -> Result<PatchTarget, PatchCommitError> {
    let fallback = format!("{name}@{version}");
    let set = patch_candidates_from_lockfile(raw_dependency, current_lockfile)
        .or_else(|_| patch_candidates_from_lockfile(&fallback, current_lockfile))
        .map_err(PatchCommitError::PatchTarget)?;
    let candidate = set
        .preferred_versions
        .iter()
        .chain(set.versions.iter())
        .find(|candidate| candidate.version == version)
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
        apply_to_all,
        git_tarball_url: candidate.git_tarball_url.clone(),
        package_key: candidate.package_key.clone(),
    })
}

fn resolve_path(dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { dir.join(path) }
}

fn clean_source_dir(state: &State, patch_dir: &Path) -> PathBuf {
    let hash = create_short_hash(&patch_dir.to_string_lossy());
    state.config.store_dir.tmp().join("patch-commit").join(hash)
}

fn cleanup_after_diff(clean_dir: &Path, filtered: &PkgFilesForDiff) {
    remove_dir_if_exists(clean_dir);
    if let PkgFilesForDiff::Temporary(path) = filtered {
        remove_dir_if_exists(path);
    }
}

fn remove_dir_if_exists(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            eprintln!("Failed to clean up temporary directory at {}: {error}", path.display());
        }
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
