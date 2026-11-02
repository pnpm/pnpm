//! Recreating a source symlink in an imported package.

use super::{
    ImportIndexedDirError, Placement, SymlinkTargets, clear_dir_blocking_file,
    file_matches_store_entry,
};
use pnpm_fs::{Host, StoreFile, SymlinkPlacement, SymlinkPlacementContext};
use std::{
    borrow::Cow,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

/// Where [`populate_dir`](super::populate_dir) writes a package whose
/// symlinks it preserves.
#[derive(Clone, Copy)]
pub(super) struct SymlinkRoots<'a> {
    /// The directory the entries are written into.
    pub(super) written_dir: &'a Path,
    /// The directory `written_dir` ends up at. A Windows junction holds
    /// an absolute target, so it has to point into this one.
    pub(super) final_dir: &'a Path,
    /// The imported entries and their directories, relative to the
    /// package, as [`imported_paths`] lists them.
    pub(super) imported: &'a HashSet<&'a str>,
    pub(super) policy: SymlinkTargets,
}

/// Every key of `cas_paths` and each directory above it, the package root
/// included as `""`.
pub(in crate::import_indexed_dir) fn imported_paths(
    cas_paths: &pnpm_fs::CasPaths,
) -> HashSet<&str> {
    pnpm_fs::selected_paths_and_ancestors(cas_paths.keys().map(String::as_str))
}

/// Whether `path` is a symlink or, on Windows, a junction.
pub(in crate::import_indexed_dir) fn is_symlink(path: &Path) -> bool {
    pnpm_fs::is_symlink(path)
}

/// Handle a symlink entry or resolve the regular-file path to import.
///
/// Returns `Ok(None)` when `store_path` was placed (or omitted) as a symlink,
/// or `Ok(Some(resolved_path))` when the caller should import `resolved_path`
/// as a regular file.
pub(super) fn place_symlink_or_resolve_file<'a>(
    placement: Placement,
    source: &'a StoreFile,
    target: &Path,
    symlinks: Option<SymlinkRoots<'_>>,
) -> Result<Option<Cow<'a, StoreFile>>, ImportIndexedDirError> {
    let Some(roots) = symlinks else {
        return Ok(Some(Cow::Borrowed(source)));
    };
    let store_path = source.path.as_path();
    if !is_symlink(store_path) {
        return Ok(Some(Cow::Borrowed(source)));
    }
    if place_symlink_entry(placement, store_path, target, roots)? {
        return Ok(None);
    }
    let resolved =
        fs::canonicalize(store_path).map_err(|error| import_link_error(store_path, target, error))?;
    Ok(Some(Cow::Owned(StoreFile::new(resolved))))
}

/// Recreate the symlink at `store_path` as `target`, reporting whether the
/// entry is handled. A link that [`SymlinkRoots::policy`] leaves out is
/// not recreated: a file link is left for the caller to import as a file,
/// and a directory link is left out.
pub(super) fn place_symlink_entry(
    placement: Placement,
    store_path: &Path,
    target: &Path,
    roots: SymlinkRoots<'_>,
) -> Result<bool, ImportIndexedDirError> {
    let ctx = SymlinkPlacementContext {
        dest_root: roots.written_dir,
        selected_paths: roots.imported,
        policy: roots.policy,
    };
    let decision = pnpm_fs::resolve_symlink_placement(store_path, target, ctx)
        .map_err(|error| import_link_error(store_path, target, error))?;
    let (link_target, is_dir) = match decision {
        Ok(SymlinkPlacement::Preserve { link_target, is_dir, .. }) => (link_target, is_dir),
        Ok(SymlinkPlacement::ImportFile) => return Ok(false),
        Ok(SymlinkPlacement::Omit) => return Ok(true),
        Err(escaped) => {
            return Err(ImportIndexedDirError::SymlinkTargetEscapes {
                target: escaped,
                root: roots.written_dir.to_path_buf(),
            });
        }
    };
    if placement == Placement::Repair && file_matches_store_entry(target, store_path) {
        return Ok(true);
    }
    clear_dir_blocking_file::<Host>(target)?;
    let temp = stage_symlink(&link_target, is_dir, store_path, target, roots)?;
    commit_symlink_placement(&temp, target, store_path).map(|()| true)
}

fn stage_symlink(
    link_target: &Path,
    is_dir: bool,
    store_path: &Path,
    target: &Path,
    roots: SymlinkRoots<'_>,
) -> Result<PathBuf, ImportIndexedDirError> {
    let temp = super::super::staging::pick_stage_path(target);
    let created = if is_dir {
        let final_target = final_link_target(target, link_target, roots);
        pnpm_fs::symlink_dir_with_contents(&final_target, link_target, &temp)
    } else {
        pnpm_fs::symlink_file_or_copy(link_target, store_path, &temp)
    };
    if let Err(error) = created {
        let _ = pnpm_fs::remove_dirent(&temp);
        return Err(import_link_error(store_path, target, error));
    }
    Ok(temp)
}

fn import_link_error(
    store_path: &Path,
    target: &Path,
    error: std::io::Error,
) -> ImportIndexedDirError {
    ImportIndexedDirError::LinkFile(crate::link_file::LinkFileError::Import {
        from: store_path.to_path_buf(),
        to: target.to_path_buf(),
        error,
    })
}

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
    let (Ok(target_link), Ok(store_link)) =
        (pnpm_fs::read_symlink_dir(target), pnpm_fs::read_symlink_dir(store_path))
    else {
        return false;
    };
    target_link == pnpm_fs::relative_link_target(store_path, store_link)
}
