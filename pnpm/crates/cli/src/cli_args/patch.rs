#[cfg(target_family = "wasm")]
use crate::dialoguer_wasm::Select;
use crate::{
    State,
    cli_args::patch_state::{EditDirState, StateFileError, write_edit_dir_state},
};
use clap::Args;
use derive_more::{Display, Error};
#[cfg(not(target_family = "wasm"))]
use dialoguer::Select;
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
    ) -> miette::Result<()> {
        let PatchArgs {
            package_name,
            edit_dir,
            ignore_existing,
        } = self;
        let package_name = package_name.ok_or(PatchError::MissingPackageName)?;
        validate_custom_edit_dir(dir, edit_dir.as_deref())?;
        let workspace_projects = dedicated_workspace_projects(state.config)?;
        let current_lockfile =
            load_current_lockfile_for_patch(state.config, &workspace_projects, &package_name)
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

fn validate_custom_edit_dir(dir: &Path, edit_dir: Option<&Path>) -> Result<(), PatchError> {
    if let Some(edit_dir) = edit_dir.map(|path| resolve_path(dir, path)) {
        reject_edit_dir_symlink_components_under(dir, &edit_dir)?;
        reject_non_empty_custom_edit_dir(&edit_dir)?;
    }
    Ok(())
}

pub(super) fn dedicated_workspace_projects(
    config: &pnpm_config::Config,
) -> miette::Result<Vec<pnpm_workspace::Project>> {
    if config.shares_one_lockfile() {
        return Ok(Vec::new());
    }
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(Vec::new());
    };
    let (projects, _) = super::recursive::discover_workspace_projects(workspace_dir, config)?;
    Ok(projects)
}

pub(super) fn load_current_lockfile_for_patch(
    config: &pnpm_config::Config,
    workspace_projects: &[pnpm_workspace::Project],
    raw_dependency: &str,
) -> Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError> {
    let active = Lockfile::load_current_from_install_state_dir(&config.install_state_dir)?;
    if workspace_projects.is_empty()
        || active
            .as_ref()
            .is_some_and(|lockfile| {
                patch_candidates_from_lockfile(raw_dependency, lockfile).is_ok()
            })
    {
        return Ok(active);
    }
    merge_workspace_current_lockfiles(config, workspace_projects, active)
}

fn merge_workspace_current_lockfiles(
    config: &pnpm_config::Config,
    projects: &[pnpm_workspace::Project],
    mut merged: Option<Lockfile>,
) -> Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError> {
    for project in projects {
        if let Some(project_lockfile) = load_project_current_lockfile(config, project)? {
            merge_lockfile_packages_and_snapshots(&mut merged, project_lockfile);
        }
    }
    Ok(merged)
}

fn load_project_current_lockfile(
    config: &pnpm_config::Config,
    project: &pnpm_workspace::Project,
) -> Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError> {
    let mut project_config = config.clone();
    let project_name = super::pipelines::project_manifest_name(project);
    project_config.anchor_dedicated_project(&project.root_dir, project_name);
    Lockfile::load_current_from_install_state_dir(&project_config.install_state_dir)
}

fn merge_lockfile_packages_and_snapshots(merged: &mut Option<Lockfile>, other: Lockfile) {
    let Some(acc) = merged else {
        *merged = Some(other);
        return;
    };
    if let Some(packages) = other.packages {
        acc.packages.get_or_insert_with(Default::default).extend(packages);
    }
    if let Some(snapshots) = other.snapshots {
        acc.snapshots.get_or_insert_with(Default::default).extend(snapshots);
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
        Select::new()
            .with_prompt("Choose which version to patch")
            .items(&labels)
            .default(0)
            .interact()
            .into_diagnostic()
            .map_err(|err| miette!("patch version selection failed: {err}"))
            .map_err(|_| PatchError::Canceled)
    }

    fn confirm_apply_to_all(&self) -> Result<bool, PatchError> {
        crate::confirm_prompt::confirm("Apply this patch to all versions?", None)
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
    pnpm_diagnostics::print!("{}", render_success(edit_dir, io::stdout().is_terminal()));
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
