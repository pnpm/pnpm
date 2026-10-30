//! The mode a file materialized into `node_modules` carries.

#[cfg(any(unix, target_os = "wasi"))]
use dashmap::DashSet;
#[cfg(unix)]
use std::fs;
use std::{io, path::Path};
#[cfg(any(unix, target_os = "wasi"))]
use std::{path::PathBuf, sync::LazyLock};

/// The permissions the copy tier gives a copy of `source_file`: the
/// store-entry mode for a store entry, or the source's own permissions.
pub(super) fn desired_permissions(source_file: &Path) -> io::Result<pnpm_fs::CopyPermissions> {
    #[cfg(any(unix, target_os = "wasi"))]
    if let Some(mode) = desired_mode(source_file) {
        return Ok(pnpm_fs::CopyPermissions::from_mode(mode.mode()));
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
fn desired_mode(source_file: &Path) -> Option<pnpm_fs::FileMode> {
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
/// A linkable answer is remembered for the rest of the process, so a store
/// entry linked into many packages is stat'ed once (pnpm/pnpm#17500). A
/// later change to the entry's mode, such as a bin's exec bit set through
/// one of its hardlinks, does not reach the answer.
#[cfg(any(unix, target_os = "wasi"))]
pub(super) fn source_has_desired_mode(source_file: &Path) -> io::Result<bool> {
    static LINKABLE_SOURCES: LazyLock<DashSet<PathBuf>> = LazyLock::new(DashSet::new);
    let Some(desired) = desired_mode(source_file) else { return Ok(true) };
    if LINKABLE_SOURCES.contains(source_file) {
        return Ok(true);
    }
    let linkable =
        pnpm_fs::file_mode::store_inode_mode_is_linkable(source_mode(source_file)?, desired);
    if linkable {
        LINKABLE_SOURCES.insert(source_file.to_path_buf());
    }
    Ok(linkable)
}

/// The mode the store entry at `source_file` carries right now. WASI reports
/// it through the host rather than through metadata.
#[cfg(unix)]
fn source_mode(source_file: &Path) -> io::Result<pnpm_fs::FileMode> {
    Ok(pnpm_fs::FileMode::from_permissions(&fs::metadata(source_file)?.permissions()))
}

#[cfg(target_os = "wasi")]
fn source_mode(source_file: &Path) -> io::Result<pnpm_fs::FileMode> {
    Ok(pnpm_fs::FileMode::from_mode(pnpm_fs::copy_permissions(source_file)?.mode()))
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
