//! The mode a file materialized into `node_modules` carries.

#[cfg(unix)]
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock, PoisonError},
};
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
/// The answer is remembered per source path ([`linkable_sources`]), so a
/// blob shared by many packages is `stat`ed once rather than once per link
/// (pnpm/pnpm#17500).
#[cfg(unix)]
pub(super) fn source_has_desired_mode(source_file: &Path) -> io::Result<bool> {
    let Some(desired) = desired_mode(source_file) else { return Ok(true) };
    let cached = linkable_sources()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(source_file)
        .copied();
    if let Some(linkable) = cached {
        return Ok(linkable);
    }
    let mode = source_mode(source_file)?;
    let linkable = pnpm_fs::file_mode::store_inode_mode_is_linkable(mode, desired);
    linkable_sources()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(source_file.to_path_buf(), linkable);
    Ok(linkable)
}

/// The linkability of every store blob asked about so far, by source path
/// (pnpm/pnpm#17500).
///
/// A store blob's mode is a property of the blob, not of the target it is
/// linked to, and a blob shared by many packages — a `LICENSE`, say — was
/// `stat`ed once per link. Nothing can change a blob's mode under this
/// cache: `current_umask()` is read once per process, pnpm writes each
/// content-addressed path once, and an import only ever `chmod`s a target
/// it does not share an inode with ([`align_target_mode`] runs after a
/// reflink, and [`super::recover_from_concurrent_import`] skips a target
/// [`super::same_inode`] finds shared).
///
/// A `stat` that fails is not remembered: the blob may still be arriving
/// from another process. Concurrent workers can both miss the same blob
/// and both `stat` it — bounded by the worker count, not by the file count.
#[cfg(unix)]
fn linkable_sources() -> &'static Mutex<HashMap<PathBuf, bool>> {
    static SOURCES: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    SOURCES.get_or_init(Mutex::default)
}

/// The mode bits of `source_file`, counting the `stat` so a test can hold
/// the hardlink tiers to one per blob (pnpm/pnpm#17500).
#[cfg(unix)]
fn source_mode(source_file: &Path) -> io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    #[cfg(test)]
    SOURCE_MODE_STATS.with(|count| count.set(count.get() + 1));
    Ok(fs::metadata(source_file)?.mode())
}

// `stat` calls `source_has_desired_mode` has made on this thread, kept per
// thread so a test reads its own imports and not a parallel test's.
#[cfg(all(unix, test))]
thread_local! {
    static SOURCE_MODE_STATS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The [`SOURCE_MODE_STATS`] count, for the tests that hold the hardlink
/// tiers to one `stat` per blob (pnpm/pnpm#17500).
#[cfg(all(unix, test))]
pub(super) fn source_mode_stats() -> usize {
    SOURCE_MODE_STATS.with(std::cell::Cell::get)
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

#[cfg(target_os = "wasi")]
pub(super) fn source_has_desired_mode(source_file: &Path) -> io::Result<bool> {
    let Some(desired) = desired_mode(source_file) else { return Ok(true) };
    let mode = pnpm_fs::copy_permissions(source_file)?.mode();
    Ok(pnpm_fs::file_mode::store_inode_mode_is_linkable(mode, desired))
}
