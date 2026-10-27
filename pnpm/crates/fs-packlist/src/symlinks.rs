//! Which symlinks a packed package keeps: those whose target stays inside it.

use std::{fs, path::Path};

use pnpm_fs::AdmittedSymlink;

pub(super) enum PackableEntry {
    File,
    Symlink(AdmittedSymlink),
}

/// Classify the walked entry at `path`: regular files and internal symlinks
/// that [`pnpm_fs::admit_internal_symlink`] accepts.
pub(super) fn classify_entry(
    pkg_dir: &Path,
    path: &Path,
    file_type: fs::FileType,
) -> Option<PackableEntry> {
    if file_type.is_file() {
        return Some(PackableEntry::File);
    }
    if (file_type.is_symlink() || pnpm_fs::is_symlink(path))
        && let Ok(Ok(admitted)) = pnpm_fs::admit_internal_symlink(pkg_dir, path)
    {
        return Some(PackableEntry::Symlink(admitted));
    }
    None
}
