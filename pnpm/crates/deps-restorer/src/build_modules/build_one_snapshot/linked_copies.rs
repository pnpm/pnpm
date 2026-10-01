//! Removing the links that point at a skipped optional dependency.

use super::BuildOneSnapshot;
use std::path::{Path, PathBuf};

/// Remove the links to `dirs` from every project's modules directory and
/// from the hidden hoisting directory, so a skipped optional dependency is
/// absent rather than linked to nothing. Links are matched against `dirs`
/// by their canonical targets, so this must run while `dirs` still exist.
pub(super) fn unlink_project_links(context: &BuildOneSnapshot<'_>, dirs: &[PathBuf]) {
    for modules_dir in project_modules_dirs(context) {
        unlink_children(&modules_dir, dirs);
    }
    let install_state_dir =
        context.scripts.patched_engines.install_state_dir.unwrap_or_else(|| {
            context.directories.layout.package_store_dir()
        });
    unlink_children(&install_state_dir.join("node_modules"), dirs);
}

/// The `node_modules` directory of every project in the lockfile. An importer
/// key that would escape the lockfile directory is left out.
fn project_modules_dirs(context: &BuildOneSnapshot<'_>) -> Vec<PathBuf> {
    let root = context.directories.modules_dir;
    let lockfile_dir = context.directories.lockfile_dir;
    let mut dirs = vec![root.to_path_buf()];
    let Ok(modules_dir_name) = root.strip_prefix(lockfile_dir) else { return dirs };
    dirs.extend(
        context.graph.importers
            .keys()
            .filter(|importer_id| importer_id.as_str() != ".")
            .filter(|importer_id| crate::validate_importer_id(importer_id).is_ok())
            .map(|importer_id| lockfile_dir.join(importer_id).join(modules_dir_name)),
    );
    dirs
}

pub(super) fn unlink_children(dir: &Path, dirs: &[PathBuf]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if unlink_if_points(&path, dirs) {
            continue;
        }
        unlink_scope(&path, dirs);
    }
}

fn unlink_scope(path: &Path, dirs: &[PathBuf]) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return };
    let is_real_dir = std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
        && pnpm_fs::is_symlink_or_junction(path).is_ok_and(|linked| !linked);
    if !name.starts_with('@') || !is_real_dir {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let _ = unlink_if_points(&entry.path(), dirs);
    }
}

fn unlink_if_points(link: &Path, dirs: &[PathBuf]) -> bool {
    let Ok(pointed) = std::fs::read_link(link) else { return false };
    let pointed = link
        .parent()
        .unwrap_or(link)
        .join(pointed);
    let pointed = std::fs::canonicalize(&pointed).unwrap_or(pointed);
    let hits = dirs
        .iter()
        .any(|dir| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()) == pointed);
    if hits {
        let _ = std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link));
    }
    hits
}
