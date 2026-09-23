use std::{
    collections::HashMap,
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
/// Packages requiring builds are copied (`clone-or-copy`) during install,
/// so lifecycle scripts mutating their own directory do not affect the
/// store. When verifying integrity, we verify against base files first,
/// then against any recorded `side_effects` overlay. If neither matches,
/// packages that require build are recognized as built packages rather
/// than falsely reported as modified.
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
    is_built_package(dir, index)
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

fn is_built_package(dir: &Path, index: &PackageFilesIndex) -> bool {
    if index.requires_build == Some(true) {
        return true;
    }
    if index.manifest.as_ref().is_some_and(pnpm_package_manifest::manifest_requires_build) {
        return true;
    }
    if pnpm_package_manifest::files_include_install_scripts(index.files.keys()) {
        return true;
    }
    pnpm_package_manifest::pkg_requires_build(dir)
}

/// `dir` joined with a recorded in-package path, or `None` if that path is
/// anything other than a sequence of plain names.
///
/// The recorded paths come from archive entries. Extraction rejects a
/// leading separator and `..`, but not a Windows drive prefix — and
/// [`Path::join`] discards its base when the argument has one, which would
/// point the hash at a file outside the package. Rejecting here keeps that
/// decision local to the one caller that joins index keys onto a
/// directory rather than reading them out of the CAS.
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
