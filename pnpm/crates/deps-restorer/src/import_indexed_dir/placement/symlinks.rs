//! Recreating a source symlink in an imported package.

use super::{ImportIndexedDirError, Placement, clear_dir_blocking_file, file_matches_store_entry};
use pnpm_fs::Host;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
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
}

/// Every key of `cas_paths` and each directory above it, the package root
/// included as `""`.
pub(super) fn imported_paths(cas_paths: &HashMap<String, PathBuf>) -> HashSet<&str> {
    let mut imported = HashSet::from([""]);
    for entry in cas_paths.keys() {
        let mut path = entry.as_str();
        while !path.is_empty() && imported.insert(path) {
            path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    imported
}

/// Whether `path` is a symlink or, on Windows, a junction, which
/// [`fs::FileType::is_symlink`] does not report.
pub(super) fn is_symlink(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return pnpm_fs::is_symlink_or_junction(path).unwrap_or(false);
        }
    }
    metadata.file_type().is_symlink()
}

/// The target to give the recreated link at `target`: the source link's
/// own, with an absolute one made relative to the source link. Refuses a
/// target that leads out of `pkg_root`.
fn validate_symlink_target(
    store_path: &Path,
    target: &Path,
    pkg_root: &Path,
) -> Result<PathBuf, ImportIndexedDirError> {
    let link_target = pnpm_fs::read_symlink_dir(store_path)
        .map(|link| relative_to_link(store_path, link))
        .map_err(|error| {
            ImportIndexedDirError::LinkFile(crate::link_file::LinkFileError::Import {
                from: store_path.to_path_buf(),
                to: target.to_path_buf(),
                error,
            })
        })?;
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

/// `link`, a link target read from `link_path`, with an absolute one made
/// relative to the link's directory. Both sides are canonicalized first:
/// the directory fetcher lists links under the canonical package root,
/// while the link text may reach the same place through another path.
fn relative_to_link(link_path: &Path, link: PathBuf) -> PathBuf {
    let (true, Some(parent)) = (link.is_absolute(), link_path.parent()) else {
        return link;
    };
    let parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let link = fs::canonicalize(&link).unwrap_or(link);
    pathdiff::diff_paths(&link, &parent).unwrap_or(link)
}

/// Recreate the symlink at `store_path` as `target`, reporting whether the
/// entry is handled. A link whose target the import leaves out is not
/// recreated: a file link is left for the caller to import as a file, and
/// a directory link is left out.
pub(super) fn place_symlink_entry(
    placement: Placement,
    store_path: &Path,
    target: &Path,
    roots: SymlinkRoots<'_>,
) -> Result<bool, ImportIndexedDirError> {
    let link_target = validate_symlink_target(store_path, target, roots.written_dir)?;
    let is_dir = fs::metadata(store_path).is_ok_and(|meta| meta.is_dir());
    if !imports_link_target(target, &link_target, roots) {
        return Ok(is_dir);
    }
    if placement == Placement::Repair && file_matches_store_entry(target, store_path) {
        return Ok(true);
    }
    clear_dir_blocking_file::<Host>(target)?;
    let temp = super::super::staging::pick_stage_path(target);
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
    commit_symlink_placement(&temp, target, store_path).map(|()| true)
}

fn imports_link_target(target: &Path, link_target: &Path, roots: SymlinkRoots<'_>) -> bool {
    let written_dir = pnpm_fs::lexical_normalize(roots.written_dir);
    let link_dir = target.parent().unwrap_or(roots.written_dir);
    let resolved = pnpm_fs::lexical_normalize(&link_dir.join(link_target));
    resolved
        .strip_prefix(&written_dir)
        .ok()
        .and_then(Path::to_str)
        .is_some_and(|rel| roots.imported.contains(rel.replace('\\', "/").as_str()))
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
    let (Ok(target_link), Ok(store_link)) =
        (pnpm_fs::read_symlink_dir(target), pnpm_fs::read_symlink_dir(store_path))
    else {
        return false;
    };
    target_link == relative_to_link(store_path, store_link)
}
