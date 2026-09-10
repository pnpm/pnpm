use super::{
    Component, PatchError, PatchTarget, Path, PathBuf, fs, io, is_subdir, lexical_normalize,
};

pub(super) fn resolve_path(dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { dir.join(path) }
}

pub(super) fn reject_non_empty_custom_edit_dir(edit_dir: &Path) -> Result<(), PatchError> {
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

pub(super) fn reject_non_empty_edit_dir(edit_dir: &Path) -> Result<(), PatchError> {
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

pub(super) fn default_edit_dir(
    modules_dir: &Path,
    package_name: &str,
    target: &PatchTarget,
) -> PathBuf {
    modules_dir.join(".pnpm_patches").join(default_edit_dir_name(package_name, target))
}

pub(super) fn prepare_default_edit_dir(
    modules_dir: &Path,
    edit_dir: &Path,
) -> Result<(), PatchError> {
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

pub(super) fn reject_edit_dir_symlink_components_under(
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

pub(super) fn default_edit_dir_name(package_name: &str, target: &PatchTarget) -> String {
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

pub(super) fn apply_existing_patch_file(
    config: &pnpm_config::Config,
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
    pnpm_patching::apply_patch_to_dir(edit_dir, &patch_file_path)
        .map_err(PatchError::ApplyExistingPatch)
}

struct ExistingPatchFileContext {
    project_root: PathBuf,
    patches_dir: PathBuf,
    real_patches_dir: Option<PathBuf>,
}

impl ExistingPatchFileContext {
    pub(super) fn new(lockfile_dir: &Path, patches_dir_setting: &str) -> Result<Self, PatchError> {
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

pub(super) fn checked_existing_patch_file_path(
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
    check_existing_patch_file_kind(
        &target_path,
        target_stats.as_ref(),
        real_patches_dir.as_deref(),
        patch_file,
    )?;
    Ok(target_path)
}

/// Whether what is already at the patch path may be written to: a
/// regular file, or a symlink that resolves to one inside the patches
/// directory. A symlink out of it would let a patch write anywhere.
fn check_existing_patch_file_kind(
    target_path: &Path,
    target_stats: Option<&fs::Metadata>,
    real_patches_dir: Option<&Path>,
    patch_file: &str,
) -> Result<(), PatchError> {
    if target_stats.is_some_and(fs::Metadata::is_dir) {
        return Err(PatchError::PatchFileIsDirectory { patch_file: patch_file.to_string() });
    }
    if !target_stats.is_some_and(|stats| stats.file_type().is_symlink()) {
        if target_stats.is_some_and(|stats| !stats.is_file()) {
            return Err(PatchError::PatchFileNotRegular { patch_file: patch_file.to_string() });
        }
        return Ok(());
    }
    let real_target = dunce::canonicalize(target_path).ok();
    if real_patches_dir.is_some_and(|real_patches_dir| {
        real_target.as_ref().is_none_or(|real_target| !is_subdir(real_patches_dir, real_target))
    }) {
        return Err(PatchError::PatchFileOutsidePatchesDir { patch_file: patch_file.to_string() });
    }
    if !fs::metadata(target_path).is_ok_and(|stats| stats.is_file()) {
        return Err(PatchError::PatchFileNotRegular { patch_file: patch_file.to_string() });
    }
    Ok(())
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
