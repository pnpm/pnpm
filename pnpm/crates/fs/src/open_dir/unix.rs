//! Removing entries relative to an open directory, which a link put in the
//! directory's path cannot redirect.

use std::{
    ffi::OsStr,
    fs::File,
    io::{self, ErrorKind},
};

/// Whether opening a name as a directory failed because nothing, a file, or
/// a link is at it. `O_NOFOLLOW` refuses a link with `ELOOP` on Linux and
/// macOS and with `EMLINK` on FreeBSD.
pub(super) fn is_not_a_real_dir(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
        || matches!(error.raw_os_error(), Some(libc::ELOOP | libc::EMLINK))
}

/// Remove `dir`, opened as `name` inside `parent`, with everything in it.
pub(super) fn remove_dir_all_at(parent: &File, name: &OsStr, dir: &File) -> io::Result<()> {
    for entry in pnpm_ffi_fs::directory_entry_names(dir)? {
        remove_dirent_at(dir, &entry)?;
    }
    already_removed(pnpm_ffi_fs::remove_directory_at(parent, name))
}

/// Remove the entry `name` inside `parent`: a file or a link is unlinked, a
/// directory is emptied first.
fn remove_dirent_at(parent: &File, name: &OsStr) -> io::Result<()> {
    match pnpm_ffi_fs::open_directory_at(parent, name) {
        Ok(dir) => remove_dir_all_at(parent, name, &dir),
        Err(error) if is_not_a_real_dir(&error) => {
            already_removed(pnpm_ffi_fs::unlink_at(parent, name))
        }
        Err(error) => Err(error),
    }
}

/// A removal that finds nothing there has already happened.
fn already_removed(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
