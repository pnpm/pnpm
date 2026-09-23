use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{CafsFileInfo, PackageFilesIndex, SideEffectsDiff};

use super::verify_file_integrity;

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
                .is_some_and(|path| verify_file_integrity(&path, &file.digest, algo))
        })
}

fn index_requires_build(index: &PackageFilesIndex) -> bool {
    if index.requires_build == Some(true) {
        return true;
    }
    if index.manifest.as_ref().is_some_and(pnpm_package_manifest::manifest_requires_build) {
        return true;
    }
    pnpm_package_manifest::files_include_install_scripts(index.files.keys())
}

fn is_isolated_dir(dir: &Path, files: &HashMap<String, CafsFileInfo>) -> bool {
    !files
        .keys()
        .any(|path| join_inside(dir, path).is_some_and(|joined| is_hardlinked_file(&joined)))
}

#[cfg(unix)]
fn is_hardlinked_file(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.nlink() > 1)
}

#[cfg(windows)]
fn is_hardlinked_file(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    fs::metadata(path)
        .is_ok_and(|metadata| {
            metadata
                .number_of_links()
                .is_some_and(|links| links > 1)
        })
}

#[cfg(not(any(unix, windows)))]
fn is_hardlinked_file(_path: &Path) -> bool {
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
