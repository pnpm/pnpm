//! The pnpm-managed directories (store, cache, state, ...) that project
//! discovery never reports projects from. See
//! [`FindWorkspaceProjectsOpts::ignored_directories`](super::FindWorkspaceProjectsOpts::ignored_directories).

use super::{Path, PathBuf};
use std::path::Component;

/// Whether `path` sits under one of the directories returned by
/// [`resolve_ignored_directories`]. `path` is normalized lexically first:
/// a walk anchored at a root spelled with `.` or `..` components yields
/// paths that name a managed directory without sharing its spelling.
pub(super) fn is_under_ignored_directory(path: &Path, ignored_directories: &[PathBuf]) -> bool {
    if ignored_directories.is_empty() {
        return false;
    }
    let path = pnpm_fs::lexical_normalize(path);
    ignored_directories.iter().any(|dir| starts_with_directory(&path, dir))
}

/// [`Path::starts_with`], extended to a prefix that differs from
/// `directory` only in letter case when the filesystem resolves both
/// spellings to the same directory, as the default volumes on Windows and
/// macOS do.
fn starts_with_directory(path: &Path, directory: &Path) -> bool {
    if path.starts_with(directory) {
        return true;
    }
    let mut path_components = path.components();
    let folded_prefix_matches = directory
        .components()
        .all(|expected| {
            path_components.next().is_some_and(|actual| eq_ignoring_case(actual, expected))
        });
    folded_prefix_matches && {
        let prefix: PathBuf = path
            .components()
            .take(directory.components().count())
            .collect();
        same_file::is_same_file(prefix, directory).unwrap_or(false)
    }
}

fn eq_ignoring_case(left: Component<'_>, right: Component<'_>) -> bool {
    let (left, right) = (left.as_os_str(), right.as_os_str());
    if left.is_ascii() && right.is_ascii() {
        return left.eq_ignore_ascii_case(right);
    }
    left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
}

/// Resolve managed directories against the workspace root into lexically
/// normalized paths, dropping any that equal or contain the root: a
/// workspace checked out inside the state directory still owns its
/// projects. Directories outside the root are kept, since `../` patterns
/// can reach them.
pub(super) fn resolve_ignored_directories(
    workspace_root: &Path,
    ignored_directories: &[PathBuf],
) -> Vec<PathBuf> {
    let workspace_root = pnpm_fs::lexical_normalize(workspace_root);
    ignored_directories
        .iter()
        .map(|dir| pnpm_fs::lexical_normalize(&workspace_root.join(dir)))
        .filter(|dir| !starts_with_directory(&workspace_root, dir))
        .collect()
}
