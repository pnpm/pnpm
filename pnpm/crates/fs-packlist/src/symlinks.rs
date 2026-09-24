//! Which symlinks a packed package keeps: those whose target stays inside it.

use std::{
    fs,
    path::{Component, Path},
};

/// Whether a root entry is a file, or a symlink [`is_internal_symlink`] admits.
pub(super) fn is_admissible_root_file(pkg_dir: &Path, entry: &fs::DirEntry) -> bool {
    entry
        .file_type()
        .is_ok_and(|file_type| is_packable(pkg_dir, &entry.path(), file_type))
}

/// Whether the entry at `path` is a file, or a symlink [`is_internal_symlink`]
/// admits.
pub(super) fn is_packable(pkg_dir: &Path, path: &Path, file_type: fs::FileType) -> bool {
    file_type.is_file() || (file_type.is_symlink() && is_internal_symlink(pkg_dir, path))
}

/// Whether the link at `symlink_path` points inside `pkg_dir`, both by its
/// link text and by where it resolves on disk.
fn is_internal_symlink(pkg_dir: &Path, symlink_path: &Path) -> bool {
    let Ok(target) = fs::read_link(symlink_path) else {
        return false;
    };
    let parent = symlink_path.parent().unwrap_or(pkg_dir);
    if !target.is_absolute() && relative_target_leaves_package(pkg_dir, parent, &target) {
        return false;
    }
    let resolved = if target.is_absolute() { target } else { parent.join(&target) };
    let normalized = pnpm_fs::lexical_normalize(&resolved);
    let normalized_pkg_dir = pnpm_fs::lexical_normalize(pkg_dir);
    if !normalized.starts_with(&normalized_pkg_dir) {
        return false;
    }
    if let Ok(real_target) = fs::canonicalize(symlink_path) {
        let canonical_pkg = pkg_dir.canonicalize().unwrap_or_else(|_| pkg_dir.to_path_buf());
        if !real_target.starts_with(&canonical_pkg) {
            return false;
        }
    }
    true
}

/// Whether the relative link text `target`, read from a link in `link_dir`,
/// steps above `pkg_dir` at any point. A link such as `../pkg/file` that
/// leaves the package and comes back through its directory's name resolves
/// inside it on disk, but not once the package is extracted under another
/// name.
fn relative_target_leaves_package(pkg_dir: &Path, link_dir: &Path, target: &Path) -> bool {
    let mut depth = link_dir
        .strip_prefix(pkg_dir)
        .map_or(0, |rel| rel.components().count());
    for component in target.components() {
        match component {
            Component::ParentDir => {
                let Some(parent_depth) = depth.checked_sub(1) else {
                    return true;
                };
                depth = parent_depth;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return true,
        }
    }
    false
}
