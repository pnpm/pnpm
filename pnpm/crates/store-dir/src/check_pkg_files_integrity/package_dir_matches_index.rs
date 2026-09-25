use std::{
    collections::HashMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff};

use super::file_content_matches_digest;

/// Whether the materialized package under `dir` still matches the store
/// row it was expanded from.
///
/// This is pnpm's `dint.check`, and answers what `pnpm store status`
/// asks: has anything edited the package after it was linked out of the
/// store.
///
/// Packages requiring builds that are not restored from the side-effects
/// cache are copied (`clone-or-copy`) during install, so lifecycle scripts
/// mutating their own directory do not affect the store. When verifying
/// integrity, we verify against base files first, then against any recorded
/// `side_effects` overlay. If neither matches, packages whose pristine index
/// records that they require a build are verified as isolated copies (not
/// hardlinked to the store) rather than falsely reported as modified.
#[must_use]
pub fn package_dir_matches_index(dir: &Path, index: &PackageFilesIndex) -> bool {
    if !dir.is_dir() {
        return false;
    }
    if files_match(dir, &index.files, &index.algo) {
        return true;
    }
    if index.side_effects
        .as_ref()
        .is_some_and(|side_effects| matches_side_effects(dir, index, side_effects))
    {
        return true;
    }
    index_requires_build(index) && is_isolated_dir(dir, &index.files)
}

fn matches_side_effects(
    dir: &Path,
    index: &PackageFilesIndex,
    side_effects: &HashMap<String, SideEffectsDiff>,
) -> bool {
    side_effects.values().any(|diff| matches_one_side_effect(dir, index, diff))
}

fn matches_one_side_effect(dir: &Path, index: &PackageFilesIndex, diff: &SideEffectsDiff) -> bool {
    let mut overlaid = index.files.clone();
    if let Some(deleted) = &diff.deleted {
        for file in deleted {
            overlaid.remove(file);
        }
    }
    if let Some(added) = &diff.added {
        for (path, info) in added {
            overlaid.insert(path.clone(), info.clone());
        }
    }
    files_match(dir, &overlaid, &index.algo)
}

fn files_match(dir: &Path, files: &HashMap<String, CafsFileInfo>, algo: &str) -> bool {
    files
        .iter()
        .all(|(path, file)| {
            join_inside(dir, path)
                .is_some_and(|path| file_content_matches_digest(&path, &file.digest, algo))
        })
}

fn index_requires_build(index: &PackageFilesIndex) -> bool {
    if index.requires_build == Some(true) {
        return true;
    }
    let mut triggers = pnpm_package_manifest::files_build_triggers(index.files.keys());
    if let Some(manifest) = &index.manifest {
        triggers.read_manifest(manifest);
    }
    triggers.requires_build()
}

fn is_isolated_dir(dir: &Path, files: &HashMap<String, CafsFileInfo>) -> bool {
    let Ok(dir_metadata) = fs::symlink_metadata(dir) else {
        return false;
    };
    if dir_metadata.is_symlink() {
        return false;
    }
    files.keys().all(|rel_path| is_isolated_rel_path(dir, rel_path))
}

fn is_isolated_rel_path(dir: &Path, rel_path: &str) -> bool {
    let Some(components) = relative_components(rel_path) else {
        return false;
    };
    let mut current = dir.to_path_buf();
    for (step_index, component) in components.iter().enumerate() {
        current.push(component);
        let is_leaf = step_index == components.len() - 1;
        match check_path_component(&current, is_leaf) {
            Ok(true) => {}
            Ok(false) => break,
            Err(()) => return false,
        }
    }
    true
}

fn check_path_component(path: &Path, is_leaf: bool) -> Result<bool, ()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(()),
    };
    if metadata.is_symlink() {
        return Err(());
    }
    if is_leaf && is_hardlinked_metadata(path, &metadata) {
        return Err(());
    }
    Ok(true)
}

fn relative_components(relative: &str) -> Option<Vec<&std::ffi::OsStr>> {
    let mut components = Vec::new();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(segment) => components.push(segment),
            _ => return None,
        }
    }
    Some(components)
}

#[cfg(unix)]
fn is_hardlinked_metadata(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.is_file() && metadata.nlink() > 1
}

#[cfg(windows)]
fn is_hardlinked_metadata(path: &Path, metadata: &fs::Metadata) -> bool {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    if !metadata.is_file() {
        return false;
    }
    let Ok(file) = fs::File::open(path) else {
        return true;
    };
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` owns a valid handle and `info` points to writable storage
    // of the structure initialized by `GetFileInformationByHandle`.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) } == 0 {
        return true;
    }
    // SAFETY: a successful `GetFileInformationByHandle` initializes `info`.
    let info = unsafe { info.assume_init() };
    info.nNumberOfLinks > 1
}

#[cfg(not(any(unix, windows)))]
fn is_hardlinked_metadata(_path: &Path, _metadata: &fs::Metadata) -> bool {
    true
}

/// `dir` joined with a recorded in-package path, or `None` if that path is
/// anything other than a sequence of plain names.
fn join_inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let mut joined = dir.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(segment) => joined.push(segment),
            _ => return None,
        }
    }
    Some(joined)
}
