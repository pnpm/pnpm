use crate::{
    State,
    cli_args::patch_state::{EditDirState, StateFileError, write_edit_dir_state},
};
use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Confirm, Select};
use miette::{Diagnostic, IntoDiagnostic, miette};
use owo_colors::OwoColorize;
use paths::{
    apply_existing_patch_file, default_edit_dir, prepare_default_edit_dir,
    reject_edit_dir_symlink_components_under, reject_non_empty_custom_edit_dir,
    reject_non_empty_edit_dir, resolve_path,
};
use pnpm_fs::{is_subdir, lexical_normalize};
use pnpm_lockfile::{LoadLockfileError, Lockfile};
use pnpm_package_manager::{
    PatchCandidate, PatchCandidateSet, PatchTarget, PatchTargetError, WritePackageForPatch,
    WritePackageForPatchError, default_patch_target, patch_candidates_from_lockfile,
};
use pnpm_patching::PatchApplyError;
use pnpm_reporter::Reporter;
use std::{
    fs, io,
    io::IsTerminal,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct PatchArgs {
    /// Name of the package to patch.
    pub package_name: Option<String>,
    /// The package that needs to be modified will be extracted to this directory.
    #[clap(short = 'd', long = "edit-dir", value_name = "dir")]
    pub edit_dir: Option<PathBuf>,
    /// Ignore existing patch files when patching.
    #[clap(long = "ignore-existing")]
    pub ignore_existing: bool,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub(crate) enum PatchError {
    #[display("`pnpm patch` requires the package name")]
    #[diagnostic(code(ERR_PNPM_MISSING_PACKAGE_NAME))]
    MissingPackageName,

    #[display("The modules directory is not ready for patching")]
    #[diagnostic(code(ERR_PNPM_PATCH_NO_LOCKFILE), help("Run `pnpm install` first"))]
    PatchNoLockfile,

    #[display("The target directory already exists: '{}'", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_EXISTS))]
    PatchEditDirExists { edit_dir: PathBuf },

    #[display("The directory {} is not empty", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_EDIT_DIR_NOT_EMPTY))]
    EditDirNotEmpty { edit_dir: PathBuf },

    #[display("Unable to read the target directory '{}': {source}", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_READ))]
    ReadEditDir {
        edit_dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Unable to create the default patch edit directory '{}': {source}", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_CREATE))]
    CreateEditDir {
        edit_dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Unable to resolve the default patch edit directory '{}': {source}", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_RESOLVE))]
    ResolveEditDir {
        edit_dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("The default patch edit directory is outside node_modules: '{}'", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_OUTSIDE_MODULES_DIR))]
    EditDirOutsideModulesDir { edit_dir: PathBuf },

    #[display("The default patch edit directory must not use a symbolic link: '{}'", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_SYMLINK))]
    EditDirSymlink { edit_dir: PathBuf },

    #[display("Canceled")]
    #[diagnostic(code(ERR_PNPM_PATCH_CANCELED))]
    Canceled,

    #[diagnostic(transparent)]
    LoadLockfile(#[error(source)] LoadLockfileError),

    #[diagnostic(transparent)]
    PatchTarget(#[error(source)] PatchTargetError),

    #[diagnostic(transparent)]
    WritePackage(#[error(source)] WritePackageForPatchError),

    #[diagnostic(transparent)]
    StateFile(#[error(source)] StateFileError),

    #[diagnostic(transparent)]
    ApplyExistingPatch(#[error(source)] PatchApplyError),

    #[display("Unable to find patch file {}", patch_file_path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_NOT_FOUND))]
    PatchFileNotFound { patch_file_path: PathBuf },

    #[display("The configured patches directory is outside the project: {patches_dir}")]
    #[diagnostic(code(ERR_PNPM_PATCHES_DIR_OUTSIDE_PROJECT))]
    PatchesDirOutsideProject { patches_dir: String },

    #[display("Patch file \"{patch_file}\" is outside the configured patches directory")]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR))]
    PatchFileOutsidePatchesDir { patch_file: String },

    #[display("Patch file \"{patch_file}\" is a directory")]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_IS_DIRECTORY))]
    PatchFileIsDirectory { patch_file: String },

    #[display("Patch file \"{patch_file}\" is not a regular file")]
    #[diagnostic(code(ERR_PNPM_PATCH_FILE_NOT_REGULAR))]
    PatchFileNotRegular { patch_file: String },

    #[display("Failed to read patch file metadata for {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_READ_PATCH_FILE_METADATA))]
    ReadPatchFileMetadata {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

impl PatchArgs {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        state: State,
    ) -> Result<(), PatchError> {
        let PatchArgs { package_name, edit_dir, ignore_existing } = self;
        let package_name = package_name.ok_or(PatchError::MissingPackageName)?;
        if let Some(edit_dir) = edit_dir.as_ref().map(|path| resolve_path(dir, path)) {
            reject_edit_dir_symlink_components_under(dir, &edit_dir)?;
            reject_non_empty_custom_edit_dir(&edit_dir)?;
        }
        let current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&state.config.virtual_store_dir)
                .map_err(PatchError::LoadLockfile)?
                .ok_or(PatchError::PatchNoLockfile)?;

        let candidate_set = patch_candidates_from_lockfile(&package_name, &current_lockfile)
            .map_err(PatchError::PatchTarget)?;
        let target = select_patch_target(&candidate_set)?;
        let edit_dir = prepare_patch_edit_dir(
            dir,
            edit_dir.as_deref(),
            &state.config.modules_dir,
            &package_name,
            &target,
        )?;

        WritePackageForPatch {
            tarball_mem_cache: &state.tarball_mem_cache,
            http_client: &state.http_client,
            config: state.config,
            current_lockfile: &current_lockfile,
            target: &target,
            dest: &edit_dir,
        }
        .run::<Reporter>()
        .await
        .map_err(PatchError::WritePackage)?;

        record_edit_target(&state.config.modules_dir, &edit_dir, &package_name, &target)?;

        if !ignore_existing {
            apply_existing_patch_file(state.config, &target, &edit_dir)?;
        }

        print_success(&edit_dir);
        Ok(())
    }
}

fn select_patch_target(set: &PatchCandidateSet) -> Result<PatchTarget, PatchError> {
    if let Some(target) = default_patch_target(set) {
        return Ok(target);
    }

    select_patch_target_with_prompt(set, &DialoguerPatchPrompt)
}

trait PatchPrompt {
    fn select_version(&self, candidates: &[PatchCandidate]) -> Result<usize, PatchError>;
    fn confirm_apply_to_all(&self) -> Result<bool, PatchError>;
}

struct DialoguerPatchPrompt;

impl PatchPrompt for DialoguerPatchPrompt {
    fn select_version(&self, candidates: &[PatchCandidate]) -> Result<usize, PatchError> {
        let labels: Vec<String> = candidates
            .iter()
            .map(|candidate| match &candidate.git_tarball_url {
                Some(url) => format!("{} (Git Hosted: {url})", candidate.version),
                None => candidate.version.clone(),
            })
            .collect();
        let prompt =
            Select::new().with_prompt("Choose which version to patch").items(&labels).default(0);
        prompt
            .interact()
            .into_diagnostic()
            .map_err(|err| miette!("patch version selection failed: {err}"))
            .map_err(|_| PatchError::Canceled)
    }

    fn confirm_apply_to_all(&self) -> Result<bool, PatchError> {
        Confirm::new()
            .with_prompt("Apply this patch to all versions?")
            .interact()
            .into_diagnostic()
            .map_err(|err| miette!("patch apply-to-all confirmation failed: {err}"))
            .map_err(|_| PatchError::Canceled)
    }
}

fn select_patch_target_with_prompt(
    set: &PatchCandidateSet,
    prompt: &impl PatchPrompt,
) -> Result<PatchTarget, PatchError> {
    let selected = prompt.select_version(&set.preferred_versions)?;
    let apply_to_all = prompt.confirm_apply_to_all()?;
    Ok(target_from_candidate(set, &set.preferred_versions[selected], apply_to_all))
}

fn target_from_candidate(
    set: &PatchCandidateSet,
    candidate: &PatchCandidate,
    apply_to_all: bool,
) -> PatchTarget {
    let bare_specifier =
        candidate.git_tarball_url.clone().unwrap_or_else(|| candidate.version.clone());
    PatchTarget {
        alias: set.alias.clone(),
        version: candidate.version.clone(),
        bare_specifier,
        apply_to_all,
        git_tarball_url: candidate.git_tarball_url.clone(),
        package_key: candidate.package_key.clone(),
    }
}

fn print_success(edit_dir: &Path) {
    print!("{}", render_success(edit_dir, io::stdout().is_terminal()));
}

fn render_success(edit_dir: &Path, colors_enabled: bool) -> String {
    let edit_dir = edit_dir.display().to_string();
    let command = format!("pnpm patch-commit {}", shell_quote(&edit_dir));
    let edit_dir = if colors_enabled { edit_dir.blue().to_string() } else { edit_dir };
    let command = if colors_enabled { command.green().to_string() } else { command };
    render_success_parts(&edit_dir, &command)
}

fn shell_quote(value: &str) -> String {
    if cfg!(windows) {
        format!(r#""{}""#, value.replace('"', r#"\""#))
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

fn render_success_parts(edit_dir: &str, command: &str) -> String {
    format!(
        "Patch: You can now edit the package at:\n\n  {edit_dir}\n\nTo commit your changes, run:\n\n  {command}\n\n",
    )
}

#[cfg(test)]
mod tests;

fn prepare_patch_edit_dir(
    dir: &Path,
    custom_dir: Option<&Path>,
    modules_dir: &Path,
    package_name: &str,
    target: &PatchTarget,
) -> Result<PathBuf, PatchError> {
    let edit_dir = if let Some(path) = custom_dir {
        resolve_path(dir, path)
    } else {
        let edit_dir = default_edit_dir(modules_dir, package_name, target);
        prepare_default_edit_dir(modules_dir, &edit_dir)?;
        edit_dir
    };
    reject_non_empty_edit_dir(&edit_dir)?;

    Ok(edit_dir)
}

fn record_edit_target(
    modules_dir: &Path,
    edit_dir: &Path,
    package_name: &str,
    target: &PatchTarget,
) -> Result<(), PatchError> {
    write_edit_dir_state(
        modules_dir,
        edit_dir,
        &EditDirState {
            patched_pkg: package_name.to_owned(),
            apply_to_all: target.apply_to_all,
            package_key: Some(target.package_key.clone()),
        },
    )
    .map_err(PatchError::StateFile)
}

mod paths;
