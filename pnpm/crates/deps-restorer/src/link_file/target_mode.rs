//! The mode a file materialized into `node_modules` carries.

use std::{fs, io, path::Path};

/// The permissions the copy tier gives a copy of `source_file`: its
/// [`desired_mode`], or the source's own permissions when it has none.
pub(super) fn desired_permissions(source_file: &Path) -> io::Result<fs::Permissions> {
    #[cfg(unix)]
    if let Some(mode) = desired_mode(source_file) {
        use std::os::unix::fs::PermissionsExt;
        return Ok(fs::Permissions::from_mode(mode));
    }
    fs::File::open(source_file)?.metadata().map(|meta| meta.permissions())
}

/// The mode the materialized file should carry: what a fresh store
/// write under the current process umask would give this CAS entry
/// (pnpm/pnpm#3807).
///
/// `None` for a file of a local-directory dependency, which keeps the
/// mode its project gives it (see [`pnpm_fs::file_mode::is_cas_file_path`]).
#[cfg(unix)]
fn desired_mode(source_file: &Path) -> Option<u32> {
    pnpm_fs::file_mode::is_cas_file_path(source_file)
        .then(|| {
            pnpm_fs::file_mode::store_entry_mode(
                pnpm_fs::file_mode::cas_path_is_executable(source_file),
                pnpm_fs::file_mode::current_umask(),
            )
        })
}

/// Whether `source_file`'s on-disk mode is [`desired_mode`]. A mode that
/// differs cannot be fixed on a store inode, so the hardlink tiers copy
/// instead; a reflink that copied the same mode onto the target is
/// aligned instead.
#[cfg(unix)]
pub(super) fn source_has_desired_mode(source_file: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let Some(mode) = desired_mode(source_file) else { return Ok(true) };
    Ok(fs::metadata(source_file)?.mode() & 0o777 == mode)
}

/// Align a materialized `target_link` with the mode a fresh store write
/// under the current umask would give `source_file`. A reflink carries the
/// source's mode onto the target — `clonefile` copies its attributes and the
/// reflink tier sets them explicitly — so an entry the store wrote under a
/// wider umask keeps that wider mode in `node_modules` until it is aligned
/// here (pnpm/pnpm#3807).
#[cfg(unix)]
pub(super) fn align_target_mode(source_file: &Path, target_link: &Path) -> io::Result<()> {
    match desired_mode(source_file) {
        Some(mode) => pnpm_fs::file_mode::set_path_permissions(target_link, mode),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
pub(super) fn source_has_desired_mode(_source_file: &Path) -> io::Result<bool> {
    Ok(true)
}

#[cfg(not(unix))]
pub(super) fn align_target_mode(_source_file: &Path, _target_link: &Path) -> io::Result<()> {
    Ok(())
}
