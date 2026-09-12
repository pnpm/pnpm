use super::{
    Component, PatchCommitError, Path, PathBuf, PkgFilesForDiff, State, Write, create_short_hash,
    fs, io, is_subdir, lexical_normalize,
};

pub(super) struct PatchFileWriteContext {
    pub(super) patches_dir: PathBuf,
    pub(super) real_patches_dir: PathBuf,
}

impl PatchFileWriteContext {
    pub(super) fn new(
        lockfile_dir: &Path,
        patches_dir_setting: &str,
    ) -> Result<Self, PatchCommitError> {
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

    pub(super) fn patch_file_path(&self, patch_file: &str) -> Result<PathBuf, PatchCommitError> {
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

pub(super) fn write_patch_file_atomically(target: &Path, content: &[u8]) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
    tmp.persist(target).map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn clean_source_dir(state: &State, patch_dir: &Path) -> PathBuf {
    let hash = create_short_hash(&patch_dir.to_string_lossy());
    state.config.store_dir.tmp().join("patch-commit").join(hash)
}

pub(super) fn cleanup_after_diff(
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

pub(super) fn remove_dir_if_exists(path: &Path) -> io::Result<()> {
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

pub(super) fn normalize_patches_dir_name(input: &str) -> String {
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

pub(super) fn path_from_forward_slash(path: &str) -> PathBuf {
    path.split('/').collect()
}
