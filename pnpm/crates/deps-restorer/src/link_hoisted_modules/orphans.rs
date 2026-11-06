//! Phase 1 of the hoisted linker: clearing the directories the new plan
//! does not place.

use super::{LinkHoistedModulesError, LinkHoistedModulesOpts, containing_modules_dir};
use crate::{DepHierarchy, prune_direct_deps::remove_dep_bins};
use pnpm_fs::{read_modules_dir, rename_to_free_name};
use rayon::prelude::*;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

/// A package directory on disk that the hoisting plan does not place.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct UnplannedDir {
    dir: PathBuf,
    modules_dir: PathBuf,
    /// `dir` relative to `modules_dir`, so scoped packages keep their
    /// `@scope/` prefix.
    pkg_name: String,
}

/// Phase 1: clear every directory the new plan does not place.
///
/// A previous directory that holds an importer, the root's included, is
/// never cleared: `.modules.yaml` can record any path.
///
/// A directory the previous install recorded is pnpm's to delete. One
/// that only the on-disk scan found is not — pnpm has no record of
/// putting it there, so it is quarantined under `.ignored` instead, the
/// way an alien package directory already is. Both are best-effort with
/// the same `EPERM`/`EBUSY` tolerance: a directory we cannot clear right
/// now is no worse than leaving a stale entry, and the next install will
/// retry. Returns the count attempted, not necessarily cleared — the
/// same number pnpm reports.
pub(super) fn remove_orphans(
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<u64, LinkHoistedModulesError> {
    let lockfile_dir = opts.modules_dirs.lockfile_dir();
    let recorded_dirs: BTreeSet<&PathBuf> = opts.previous_dirs
        .iter()
        .filter(|dir| !opts.graph.contains_key(*dir))
        .filter(|dir| !holds_importer(opts, dir))
        .filter(|dir| confined(dir, lockfile_dir))
        .collect();
    let unplanned_dirs = unplanned_dirs(opts, &recorded_dirs)?;
    recorded_dirs
        .par_iter()
        .for_each(|dir| {
            if let Some(modules_dir) = containing_modules_dir(dir)
                && let Err(error) = remove_dep_bins(modules_dir, dir)
            {
                tracing::warn!(?dir, %error, "failed to remove the bins of an orphan package");
            }
            let _ = try_remove_dir(dir);
        });
    unplanned_dirs.par_iter().for_each(quarantine_dir);
    Ok((recorded_dirs.len() + unplanned_dirs.len()) as u64)
}

/// The package directories in the importers' modules directories that
/// neither the plan nor the previous install placed.
///
/// A modules directory is scanned only when it is strictly inside its
/// importer and holds no importer: `modulesDir` may point at the importer
/// itself, a parent, a directory of projects, or a directory outside the
/// workspace, and pnpm does not own the rest of what is there.
fn unplanned_dirs(
    opts: &LinkHoistedModulesOpts<'_>,
    recorded_dirs: &BTreeSet<&PathBuf>,
) -> Result<BTreeSet<UnplannedDir>, LinkHoistedModulesError> {
    let mut unplanned_dirs = BTreeSet::new();
    for (importer_dir, planned_deps) in opts.hierarchy {
        let modules_dir = opts.modules_dirs.of(importer_dir);
        if !is_strictly_inside(&modules_dir, importer_dir) || holds_importer(opts, &modules_dir) {
            continue;
        }
        for unplanned in find_unplanned_dirs(&modules_dir, planned_deps)? {
            if !recorded_dirs.contains(&unplanned.dir)
                && confined(&unplanned.dir, opts.modules_dirs.lockfile_dir())
            {
                unplanned_dirs.insert(unplanned);
            }
        }
    }
    Ok(unplanned_dirs)
}

fn holds_importer(opts: &LinkHoistedModulesOpts<'_>, dir: &Path) -> bool {
    opts.modules_dirs.holds_importer(dir)
        || opts.hierarchy
            .keys()
            .any(|importer_dir| importer_dir.starts_with(dir))
}

fn is_strictly_inside(dir: &Path, parent: &Path) -> bool {
    pnpm_fs::lexical_normalize(dir)
        .strip_prefix(pnpm_fs::lexical_normalize(parent))
        .is_ok_and(|relative| relative.components().next().is_some())
}

/// Whether `dir` sits lexically inside `confine_root`.
fn confined(dir: &Path, confine_root: &Path) -> bool {
    let confined = dir.starts_with(confine_root)
        && dir
            .components()
            .all(|part| !matches!(part, std::path::Component::ParentDir));
    if !confined {
        tracing::warn!(
            ?dir,
            ?confine_root,
            "refusing to remove an orphan directory outside the install root",
        );
    }
    confined
}

/// Move a package directory pnpm has no record of installing into the
/// `.ignored` sibling of the `node_modules` holding it.
///
/// Deleting is reserved for what the previous install recorded placing.
/// Anything else may hold work someone did by hand, so it is displaced
/// rather than destroyed, and an earlier quarantined copy is never
/// overwritten. Getting it out of `node_modules` is what makes the tree
/// correct; the bytes are incidental.
fn quarantine_dir(unplanned: &UnplannedDir) {
    let ignored_dir = unplanned.modules_dir.join(".ignored").join(&unplanned.pkg_name);
    if !make_ignored_parent(&unplanned.modules_dir, &unplanned.pkg_name) {
        tracing::warn!(
            pkg_name = %unplanned.pkg_name,
            modules_dir = ?unplanned.modules_dir,
            "not moving a package to \"node_modules/.ignored\": the destination leads outside \
             the modules directory",
        );
        return;
    }
    let Ok(Some(quarantined)) = rename_to_free_name(&unplanned.dir, &ignored_dir) else {
        return;
    };
    tracing::warn!(
        pkg_name = %unplanned.pkg_name,
        ?quarantined,
        "moving a package to \"node_modules/.ignored\": it is not in the dependency tree \
         and pnpm has no record of installing it",
    );
}

/// Create `.ignored`, and the scope directory under it when `pkg_name`
/// is scoped, refusing to descend through a level that already exists as
/// anything but a real directory.
///
/// `create_dir_all` traverses a symlink it finds on the way, so a
/// `.ignored` link would redirect the move outside the tree pnpm is
/// allowed to touch.
fn make_ignored_parent(modules_dir: &Path, pkg_name: &str) -> bool {
    let Some(ignored_dir) = make_real_dir(modules_dir, ".ignored") else { return false };
    match pkg_name.split_once('/') {
        Some((scope, _)) => make_real_dir(&ignored_dir, scope).is_some(),
        None => true,
    }
}

fn make_real_dir(parent: &Path, name: &str) -> Option<PathBuf> {
    let dir = parent.join(name);
    match fs::create_dir(&dir) {
        Ok(()) => Some(dir),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let is_real_dir = fs::symlink_metadata(&dir).is_ok_and(|metadata| metadata.is_dir());
            is_real_dir.then_some(dir)
        }
        Err(_) => None,
    }
}

/// Package directories that physically exist inside `project_dir`'s
/// `node_modules` but that the new hoisting plan does not place there.
///
/// The previous-graph diff cannot see them: an install interrupted
/// before the current lockfile and `.modules.yaml` are written leaves
/// nested copies on disk while the next install starts without a
/// previous graph, so nothing ever reclaims them
/// (<https://github.com/pnpm/pnpm/issues/13676>).
///
/// Only directories carrying a `package.json` are reported. That is the
/// marker [`import_indexed_dir()`](crate::import_indexed_dir()) writes
/// last, so a directory holding one is a package that was materialized in
/// full; a directory without
/// one is not a package, and pruning is not entitled to remove it
/// however little the hoisting plan has to say about it.
///
/// Symlinks are skipped: that is how workspace packages and `link:`
/// dependencies are attached, and they are absent from the graph by
/// design.
fn find_unplanned_dirs(
    modules_dir: &Path,
    planned_deps: &DepHierarchy,
) -> Result<Vec<UnplannedDir>, LinkHoistedModulesError> {
    let pkg_names = read_modules_dir(modules_dir)
        .map_err(|source| LinkHoistedModulesError::ReadModulesDir {
            path: modules_dir.to_path_buf(),
            source,
        })?;
    let mut unplanned = Vec::new();
    for pkg_name in pkg_names {
        let dir = modules_dir.join(&pkg_name);
        if planned_deps.0.contains_key(&dir) {
            continue;
        }
        match fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.is_dir() => {
                if dir.join("package.json").exists() {
                    unplanned.push(UnplannedDir {
                        dir,
                        modules_dir: modules_dir.to_path_buf(),
                        pkg_name,
                    });
                }
            }
            // A symlink or a file is not a package directory the plan owns.
            Ok(_) => {}
            // The entry was removed while the scan was running.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(LinkHoistedModulesError::ReadModulesDir { path: dir, source });
            }
        }
    }
    Ok(unplanned)
}

/// Single-directory rimraf with error-swallowing semantics.
/// `NotFound` is a no-op (someone else already removed it);
/// everything else (`PermissionDenied`, `Other`) is silently
/// dropped — a stale directory is less bad than a panicked install.
fn try_remove_dir(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests;
