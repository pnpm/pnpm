//! Recreating a source symlink in an imported package.

use super::{ImportIndexedDirError, Placement, clear_dir_blocking_file, file_matches_store_entry};
use pnpm_fs::Host;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Where [`populate_dir`] writes a package whose symlinks it preserves.
#[derive(Clone, Copy)]
pub(super) struct SymlinkRoots<'a> {
    /// The directory the entries are written into.
    pub(super) written_dir: &'a Path,
    /// The directory `written_dir` ends up at. A Windows junction holds
    /// an absolute target, so it has to point into this one.
    pub(super) final_dir: &'a Path,
}

pub(super) fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// The target to give the recreated link at `target`: the source link's
/// own, with an absolute one made relative to the source link. Refuses a
/// target that leads out of `pkg_root`.
fn validate_symlink_target(
    store_path: &Path,
    target: &Path,
    pkg_root: &Path,
) -> Result<PathBuf, ImportIndexedDirError> {
    let mut link_target = fs::read_link(store_path)
        .map_err(|error| {
            ImportIndexedDirError::LinkFile(crate::link_file::LinkFileError::Import {
                from: store_path.to_path_buf(),
                to: target.to_path_buf(),
                error,
            })
        })?;
    if link_target.is_absolute()
        && let Some(parent) = store_path.parent()
        && let Some(rel) = pathdiff::diff_paths(&link_target, parent)
    {
        link_target = rel;
    }
    let dest_dir = target.parent().unwrap_or(pkg_root);
    let resolved =
        if link_target.is_absolute() { link_target.clone() } else { dest_dir.join(&link_target) };
    if !pnpm_fs::is_subdir(pkg_root, &resolved) {
        return Err(ImportIndexedDirError::SymlinkTargetEscapes {
            target: link_target,
            root: pkg_root.to_path_buf(),
        });
    }
    Ok(link_target)
}

pub(super) fn place_symlink_entry(
    placement: Placement,
    store_path: &Path,
    target: &Path,
    roots: SymlinkRoots<'_>,
) -> Result<(), ImportIndexedDirError> {
    let link_target = validate_symlink_target(store_path, target, roots.written_dir)?;
    if placement == Placement::Repair && file_matches_store_entry(target, store_path) {
        return Ok(());
    }
    clear_dir_blocking_file::<Host>(target)?;
    let temp = super::super::staging::pick_stage_path(target);
    let is_dir = fs::metadata(store_path).is_ok_and(|meta| meta.is_dir());
    let created = if is_dir {
        let final_target = final_link_target(target, &link_target, roots);
        pnpm_fs::symlink_dir_with_contents(&final_target, &link_target, &temp)
    } else {
        create_file_link(&link_target, store_path, &temp)
    };
    if let Err(error) = created {
        let _ = pnpm_fs::remove_dirent(&temp);
        return Err(ImportIndexedDirError::LinkFile(crate::link_file::LinkFileError::Import {
            from: store_path.to_path_buf(),
            to: target.to_path_buf(),
            error,
        }));
    }
    commit_symlink_placement(&temp, target, store_path)
}

/// A file symlink has no junction to fall back to, so a process Windows
/// refuses symlinks gets a copy of the linked file.
fn create_file_link(link_target: &Path, store_path: &Path, temp: &Path) -> io::Result<()> {
    let result = pnpm_fs::create_symlink(link_target, temp, false);
    #[cfg(windows)]
    {
        if let Err(error) = &result
            && error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD)
        {
            return fs::copy(store_path, temp).map(drop);
        }
    }
    #[cfg(not(windows))]
    let _ = store_path;
    result
}

#[cfg(windows)]
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

/// The absolute path `link_target` resolves to from `target` once the
/// written directory has moved to its final place.
pub(super) fn final_link_target(
    target: &Path,
    link_target: &Path,
    roots: SymlinkRoots<'_>,
) -> PathBuf {
    let link_dir = target.parent().unwrap_or(roots.written_dir);
    let rel_dir = link_dir.strip_prefix(roots.written_dir).unwrap_or_else(|_| Path::new(""));
    pnpm_fs::lexical_normalize(&roots.final_dir.join(rel_dir).join(link_target))
}

fn commit_symlink_placement(
    temp: &Path,
    target: &Path,
    store_path: &Path,
) -> Result<(), ImportIndexedDirError> {
    match pnpm_fs::rename_with_retry(temp, target) {
        Ok(()) => Ok(()),
        Err(_) if file_matches_store_entry(target, store_path) => {
            let _ = pnpm_fs::remove_dirent(temp);
            Ok(())
        }
        Err(error) => {
            let _ = pnpm_fs::remove_dirent(temp);
            Err(ImportIndexedDirError::PlaceFile {
                from: temp.to_path_buf(),
                to: target.to_path_buf(),
                error,
            })
        }
    }
}

pub(super) fn symlink_matches_store_entry(target: &Path, store_path: &Path) -> bool {
    let (Ok(target_link), Ok(store_link)) = (fs::read_link(target), fs::read_link(store_path))
    else {
        return false;
    };
    if target_link == store_link {
        return true;
    }
    if store_link.is_absolute()
        && let Some(parent) = store_path.parent()
        && let Some(rel) = pathdiff::diff_paths(&store_link, parent)
    {
        return target_link == rel;
    }
    false
}
