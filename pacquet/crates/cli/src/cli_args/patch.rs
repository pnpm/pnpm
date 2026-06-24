use crate::{
    State,
    cli_args::patch_state::{EditDirState, StateFileError, write_edit_dir_state},
};
use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Confirm, Select};
use miette::{Diagnostic, IntoDiagnostic, miette};
use owo_colors::OwoColorize;
use pacquet_fs::{is_subdir, lexical_normalize};
use pacquet_lockfile::{LoadLockfileError, Lockfile};
use pacquet_package_manager::{
    PatchCandidate, PatchCandidateSet, PatchTarget, PatchTargetError, WritePackageForPatch,
    WritePackageForPatchError, default_patch_target, patch_candidates_from_lockfile,
};
use pacquet_patching::PatchApplyError;
use pacquet_reporter::Reporter;
use std::{
    fs,
    io::{self, IsTerminal},
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
pub enum PatchError {
    #[display("`pacquet patch` requires the package name")]
    #[diagnostic(code(ERR_PNPM_MISSING_PACKAGE_NAME))]
    MissingPackageName,

    #[display("The modules directory is not ready for patching")]
    #[diagnostic(code(ERR_PNPM_PATCH_NO_LOCKFILE), help("Run pacquet install first"))]
    PatchNoLockfile,

    #[display("The target directory already exists: '{}'", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_EDIT_DIR_EXISTS))]
    PatchEditDirExists { edit_dir: PathBuf },

    #[display("The directory {} is not empty", edit_dir.display())]
    #[diagnostic(code(ERR_PNPM_EDIT_DIR_NOT_EMPTY))]
    EditDirNotEmpty { edit_dir: PathBuf },

    #[display("Unable to read the target directory '{}': {source}", edit_dir.display())]
    #[diagnostic(code(pacquet::patch_edit_dir_read))]
    ReadEditDir {
        edit_dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Unable to create the default patch edit directory '{}': {source}", edit_dir.display())]
    #[diagnostic(code(pacquet::patch_edit_dir_create))]
    CreateEditDir {
        edit_dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Unable to resolve the default patch edit directory '{}': {source}", edit_dir.display())]
    #[diagnostic(code(pacquet::patch_edit_dir_resolve))]
    ResolveEditDir {
        edit_dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("The default patch edit directory is outside node_modules: '{}'", edit_dir.display())]
    #[diagnostic(code(pacquet::patch_edit_dir_outside_modules_dir))]
    EditDirOutsideModulesDir { edit_dir: PathBuf },

    #[display("The default patch edit directory must not use a symbolic link: '{}'", edit_dir.display())]
    #[diagnostic(code(pacquet::patch_edit_dir_symlink))]
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
    #[diagnostic(code(pacquet::patch_read_patch_file_metadata))]
    ReadPatchFileMetadata {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

impl PatchArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
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
        let edit_dir = if let Some(path) = edit_dir.as_ref() {
            resolve_path(dir, path)
        } else {
            let edit_dir = default_edit_dir(&state.config.modules_dir, &package_name, &target);
            prepare_default_edit_dir(&state.config.modules_dir, &edit_dir)?;
            edit_dir
        };
        reject_non_empty_edit_dir(&edit_dir)?;

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

        write_edit_dir_state(
            &state.config.modules_dir,
            &edit_dir,
            &EditDirState {
                patched_pkg: package_name.clone(),
                apply_to_all: target.apply_to_all,
                package_key: Some(target.package_key.clone()),
            },
        )
        .map_err(PatchError::StateFile)?;

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

fn resolve_path(dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { dir.join(path) }
}

fn reject_non_empty_custom_edit_dir(edit_dir: &Path) -> Result<(), PatchError> {
    reject_edit_dir_symlink_if_exists(edit_dir)?;
    if !edit_dir.exists() {
        return Ok(());
    }
    if is_empty_dir(edit_dir)
        .map_err(|source| PatchError::ReadEditDir { edit_dir: edit_dir.to_path_buf(), source })?
    {
        return Ok(());
    }
    Err(PatchError::PatchEditDirExists { edit_dir: edit_dir.to_path_buf() })
}

fn reject_non_empty_edit_dir(edit_dir: &Path) -> Result<(), PatchError> {
    reject_edit_dir_symlink_if_exists(edit_dir)?;
    if !edit_dir.exists() {
        return Ok(());
    }
    if is_empty_dir(edit_dir)
        .map_err(|source| PatchError::ReadEditDir { edit_dir: edit_dir.to_path_buf(), source })?
    {
        return Ok(());
    }
    Err(PatchError::EditDirNotEmpty { edit_dir: edit_dir.to_path_buf() })
}

fn is_empty_dir(path: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(path)?.next().is_none())
}

fn default_edit_dir(modules_dir: &Path, package_name: &str, target: &PatchTarget) -> PathBuf {
    modules_dir.join(".pnpm_patches").join(default_edit_dir_name(package_name, target))
}

fn prepare_default_edit_dir(modules_dir: &Path, edit_dir: &Path) -> Result<(), PatchError> {
    let edit_root = modules_dir.join(".pnpm_patches");
    if edit_dir == edit_root || !is_subdir(&edit_root, edit_dir) {
        return Err(PatchError::EditDirOutsideModulesDir { edit_dir: edit_dir.to_path_buf() });
    }
    reject_edit_dir_symlink_if_exists(&edit_root)?;
    fs::create_dir_all(&edit_root)
        .map_err(|source| PatchError::CreateEditDir { edit_dir: edit_root.clone(), source })?;
    reject_default_edit_dir_symlink_components(&edit_root, edit_dir)?;

    let real_modules_dir = dunce::canonicalize(modules_dir).map_err(|source| {
        PatchError::ResolveEditDir { edit_dir: modules_dir.to_path_buf(), source }
    })?;
    let real_edit_root = dunce::canonicalize(&edit_root)
        .map_err(|source| PatchError::ResolveEditDir { edit_dir: edit_root.clone(), source })?;
    if !is_subdir(&real_modules_dir, &real_edit_root) {
        return Err(PatchError::EditDirOutsideModulesDir { edit_dir: edit_root });
    }
    Ok(())
}

fn reject_default_edit_dir_symlink_components(
    edit_root: &Path,
    edit_dir: &Path,
) -> Result<(), PatchError> {
    reject_edit_dir_symlink_if_exists(edit_root)?;
    let relative = edit_dir
        .strip_prefix(edit_root)
        .map_err(|_| PatchError::EditDirOutsideModulesDir { edit_dir: edit_dir.to_path_buf() })?;
    let mut current = edit_root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                current.push(part);
                match fs::symlink_metadata(&current) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(PatchError::EditDirSymlink { edit_dir: current });
                    }
                    Ok(_) => {}
                    Err(source) if source.kind() == io::ErrorKind::NotFound => break,
                    Err(source) => {
                        return Err(PatchError::ReadEditDir { edit_dir: current, source });
                    }
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PatchError::EditDirOutsideModulesDir {
                    edit_dir: edit_dir.to_path_buf(),
                });
            }
        }
    }
    Ok(())
}

fn reject_edit_dir_symlink_if_exists(path: &Path) -> Result<(), PatchError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(PatchError::EditDirSymlink { edit_dir: path.to_path_buf() })
        }
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PatchError::ReadEditDir { edit_dir: path.to_path_buf(), source }),
    }
}

fn reject_edit_dir_symlink_components_under(
    root: &Path,
    edit_dir: &Path,
) -> Result<(), PatchError> {
    let root = lexical_normalize(root);
    let edit_dir = lexical_normalize(edit_dir);
    if !is_subdir(&root, &edit_dir) {
        return Ok(());
    }
    reject_default_edit_dir_symlink_components(&root, &edit_dir)
}

fn default_edit_dir_name(package_name: &str, target: &PatchTarget) -> String {
    if !target.alias.is_empty() && !target.bare_specifier.is_empty() {
        return format!("{}@{}", target.alias, sanitize_bare_specifier(&target.bare_specifier));
    }
    if !target.alias.is_empty() {
        return target.alias.clone();
    }
    package_name.to_string()
}

fn sanitize_bare_specifier(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut replacing = false;
    for ch in input.chars() {
        if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            if !replacing {
                result.push('+');
                replacing = true;
            }
        } else {
            result.push(ch);
            replacing = false;
        }
    }
    result
}

fn apply_existing_patch_file(
    config: &pacquet_config::Config,
    target: &PatchTarget,
    edit_dir: &Path,
) -> Result<(), PatchError> {
    let Some(patched_dependencies) = &config.patched_dependencies else { return Ok(()) };
    let exact_key = format!("{}@{}", target.alias, target.bare_specifier);
    let patch_file = patched_dependencies
        .get(&exact_key)
        .or_else(|| target.apply_to_all.then(|| patched_dependencies.get(&target.alias)).flatten());
    let Some(patch_file) = patch_file else { return Ok(()) };
    let base_dir = config
        .workspace_dir
        .as_deref()
        .unwrap_or_else(|| config.modules_dir.parent().unwrap_or_else(|| Path::new(".")));
    let patch_file_path = checked_existing_patch_file_path(
        base_dir,
        config.patches_dir.as_deref().unwrap_or("patches"),
        patch_file,
    )?;
    if !patch_file_path.exists() {
        return Err(PatchError::PatchFileNotFound { patch_file_path });
    }
    pacquet_patching::apply_patch_to_dir(edit_dir, &patch_file_path)
        .map_err(PatchError::ApplyExistingPatch)
}

struct ExistingPatchFileContext {
    project_root: PathBuf,
    patches_dir: PathBuf,
    real_patches_dir: Option<PathBuf>,
}

impl ExistingPatchFileContext {
    fn new(lockfile_dir: &Path, patches_dir_setting: &str) -> Result<Self, PatchError> {
        let project_root = lexical_normalize(lockfile_dir);
        let real_project_root =
            dunce::canonicalize(&project_root).unwrap_or_else(|_| project_root.clone());
        let patches_dir = join_setting_path(&project_root, patches_dir_setting);
        if !is_subdir(&project_root, &patches_dir) {
            return Err(PatchError::PatchesDirOutsideProject {
                patches_dir: patches_dir_setting.to_string(),
            });
        }
        let real_patches_dir = realpath_if_exists(&patches_dir);
        if real_patches_dir.as_ref().is_some_and(|real| !is_subdir(&real_project_root, real)) {
            return Err(PatchError::PatchesDirOutsideProject {
                patches_dir: patches_dir_setting.to_string(),
            });
        }
        Ok(Self { project_root, patches_dir, real_patches_dir })
    }
}

fn checked_existing_patch_file_path(
    lockfile_dir: &Path,
    patches_dir_setting: &str,
    patch_file: &str,
) -> Result<PathBuf, PatchError> {
    let ctx = ExistingPatchFileContext::new(lockfile_dir, patches_dir_setting)?;
    let target_path = resolve_patch_path(&ctx.project_root, Path::new(patch_file));
    if target_path == ctx.patches_dir || !is_subdir(&ctx.patches_dir, &target_path) {
        return Err(PatchError::PatchFileOutsidePatchesDir { patch_file: patch_file.to_string() });
    }

    let parent_dir = target_path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let target_stats = lstat_patch_if_exists(&target_path)?;
    let real_parent_dir = realpath_if_exists(&parent_dir);
    let real_patches_dir = ctx.real_patches_dir.or_else(|| realpath_if_exists(&ctx.patches_dir));
    if let (Some(real_parent_dir), Some(real_patches_dir)) = (&real_parent_dir, &real_patches_dir)
        && !is_subdir(real_patches_dir, real_parent_dir)
    {
        return Err(PatchError::PatchFileOutsidePatchesDir { patch_file: patch_file.to_string() });
    }
    if target_stats.as_ref().is_some_and(fs::Metadata::is_dir) {
        return Err(PatchError::PatchFileIsDirectory { patch_file: patch_file.to_string() });
    }
    if target_stats.as_ref().is_some_and(|stats| stats.file_type().is_symlink()) {
        let real_target = dunce::canonicalize(&target_path).ok();
        if real_patches_dir.as_ref().is_some_and(|real_patches_dir| {
            real_target.as_ref().is_none_or(|real_target| !is_subdir(real_patches_dir, real_target))
        }) {
            return Err(PatchError::PatchFileOutsidePatchesDir {
                patch_file: patch_file.to_string(),
            });
        }
        let is_regular_file = fs::metadata(&target_path).is_ok_and(|stats| stats.is_file());
        if !is_regular_file {
            return Err(PatchError::PatchFileNotRegular { patch_file: patch_file.to_string() });
        }
    } else if target_stats.as_ref().is_some_and(|stats| !stats.is_file()) {
        return Err(PatchError::PatchFileNotRegular { patch_file: patch_file.to_string() });
    }
    Ok(target_path)
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

fn lstat_patch_if_exists(path: &Path) -> Result<Option<fs::Metadata>, PatchError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PatchError::ReadPatchFileMetadata { path: path.to_path_buf(), source }),
    }
}

fn realpath_if_exists(path: &Path) -> Option<PathBuf> {
    dunce::canonicalize(path).ok()
}

fn print_success(edit_dir: &Path) {
    print!("{}", render_success(edit_dir, io::stdout().is_terminal()));
}

fn render_success(edit_dir: &Path, colors_enabled: bool) -> String {
    let edit_dir = edit_dir.display().to_string();
    let command = format!("pacquet patch-commit {}", shell_quote(&edit_dir));
    let edit_dir = if colors_enabled { edit_dir.blue().to_string() } else { edit_dir };
    let command = if colors_enabled { command.green().to_string() } else { command };
    render_success_parts(&edit_dir, &command)
}

fn shell_quote(value: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", value.replace('"', r#"\""#))
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
