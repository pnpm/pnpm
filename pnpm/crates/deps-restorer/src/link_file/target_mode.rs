//! The mode a file materialized into `node_modules` carries.

#[cfg(unix)]
use std::fs;
use std::{io, path::Path};

/// The permissions the copy tier gives a copy of `source_file`: the
/// store-entry mode for a store entry, or the source's own permissions.
pub(super) fn desired_permissions(source_file: &Path) -> io::Result<pnpm_fs::CopyPermissions> {
    #[cfg(any(unix, target_os = "wasi"))]
    if let Some(mode) = desired_mode(source_file) {
        return Ok(pnpm_fs::CopyPermissions::from_mode(mode));
    }
    pnpm_fs::copy_permissions(source_file)
}

/// The mode the materialized file should carry: what a fresh store
/// write under the current process umask would give this CAS entry
/// (pnpm/pnpm#3807).
///
/// `None` for a file of a local-directory dependency, which keeps the
/// mode its project gives it (see [`pnpm_fs::file_mode::is_cas_file_path`]).
#[cfg(any(unix, target_os = "wasi"))]
fn desired_mode(source_file: &Path) -> Option<u32> {
    pnpm_fs::file_mode::is_cas_file_path(source_file)
        .then(|| {
            pnpm_fs::file_mode::store_entry_mode(
                pnpm_fs::file_mode::cas_path_is_executable(source_file),
                pnpm_fs::file_mode::current_umask(),
            )
        })
}

/// Whether `source_file`'s on-disk mode is linkable where [`desired_mode`]
/// is wanted (see [`pnpm_fs::file_mode::store_inode_mode_is_linkable`]). A
/// mode that is not cannot be fixed on a store inode, so the hardlink tiers
/// copy instead; a reflink that copied the same mode onto the target is
/// aligned instead.
///
/// The entry's mode is read at most once per process (pnpm/pnpm#17500):
/// the store verifier records the mode of every entry it passes, and a
/// mode read here is recorded too, so a store entry linked into many
/// packages is not stat'ed again. A later change to the entry's mode, such
/// as a bin's exec bit set through one of its hardlinks, does not reach
/// the recorded mode.
#[cfg(any(unix, target_os = "wasi"))]
pub(super) fn source_has_desired_mode(source_file: &Path) -> io::Result<bool> {
    let Some(desired) = desired_mode(source_file) else { return Ok(true) };
    let mode = if let Some(mode) = pnpm_fs::file_mode::observed_store_inode_mode(source_file) {
        mode
    } else {
        let mode = source_mode(source_file)?;
        pnpm_fs::file_mode::record_store_inode_mode(source_file, mode);
        mode
    };
    Ok(pnpm_fs::file_mode::store_inode_mode_is_linkable(mode, desired))
}

#[cfg(unix)]
fn source_mode(source_file: &Path) -> io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(source_file)?.mode())
}

#[cfg(target_os = "wasi")]
fn source_mode(source_file: &Path) -> io::Result<u32> {
    Ok(pnpm_fs::copy_permissions(source_file)?.mode())
}

/// Align a materialized `target_link` with the mode a fresh store write
/// under the current umask would give `source_file`. A reflink carries the
/// source's mode onto the target — `clonefile` copies its attributes and the
/// reflink tier sets them explicitly — so an entry the store wrote under a
/// wider umask keeps that wider mode in `node_modules` until it is aligned
/// here (pnpm/pnpm#3807).
#[cfg(any(unix, target_os = "wasi"))]
pub(super) fn align_target_mode(source_file: &Path, target_link: &Path) -> io::Result<()> {
    match desired_mode(source_file) {
        Some(mode) => pnpm_fs::file_mode::set_path_permissions(target_link, mode),
        None => Ok(()),
    }
}

#[cfg(not(any(unix, target_os = "wasi")))]
pub(super) fn source_has_desired_mode(_source_file: &Path) -> io::Result<bool> {
    Ok(true)
}

#[cfg(not(any(unix, target_os = "wasi")))]
pub(super) fn align_target_mode(_source_file: &Path, _target_link: &Path) -> io::Result<()> {
    Ok(())
}
