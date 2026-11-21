//! Operating on entries of a directory held open, which a link put in the
//! directory's path cannot redirect.
//!
//! Every foreign call these make lives in `pnpm-ffi-fs`; what is here is
//! the policy around them: which errors mean "nothing of that kind is
//! there", and what a caller is told about each outcome.

pub(super) use self::write::write_file_at;
mod write;

use super::{
    ChildFileStat,
    child_ops::{
        ensure_within_max_bytes,
        read_bounded_bytes,
    },
};
use pnpm_ffi_fs::EntryKind;
use std::{
    ffi::{
        OsStr,
        OsString,
    },
    fs::File,
    io::{
        self,
        ErrorKind,
    },
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
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// The names `dir` holds.
pub(super) fn entry_names(dir: &File) -> io::Result<Vec<OsString>> {
    pnpm_ffi_fs::directory_entry_names(dir)
}

/// What the regular file `name` inside `parent` reports, or `None` when the
/// name holds nothing or holds something that is not a regular file.
pub(super) fn stat_file_at(parent: &File, name: &OsStr) -> io::Result<Option<ChildFileStat>> {
    let Some(entry) = pnpm_ffi_fs::entry_metadata_at(parent, name)? else {
        return Ok(None);
    };
    if entry.kind != EntryKind::File {
        return Ok(None);
    }
    Ok(Some(ChildFileStat { len: entry.len, modified: entry.modified, accessed: entry.accessed }))
}

/// Whether `name` inside `parent` is a symlink, without following it.
pub(super) fn is_symlink_at(parent: &File, name: &OsStr) -> io::Result<bool> {
    Ok(pnpm_ffi_fs::entry_metadata_at(parent, name)?
        .is_some_and(|entry| entry.kind == EntryKind::Symlink))
}

/// Read the regular file `name` inside `parent`, up to `max_bytes`.
///
/// `None` where the name holds nothing, or holds something that is not a
/// regular file. The length is checked twice, because the first answer is
/// only the one the name had before the open.
pub(super) fn read_file_at(
    parent: &File,
    name: &OsStr,
    max_bytes: usize,
) -> io::Result<Option<Vec<u8>>> {
    let Some(stat) = stat_file_at(parent, name)? else {
        return Ok(None);
    };
    ensure_within_max_bytes(stat.len, max_bytes)?;
    // The non-blocking open is what lets a FIFO or a device at this name be
    // rejected on its type: a blocking open of one would wait for a writer.
    // `O_NONBLOCK` is left set, having no effect on reads of the regular
    // file the check below requires.
    let file = match pnpm_ffi_fs::open_file_at_without_blocking(parent, name) {
        Ok(file) => file,
        Err(error) if is_non_regular_open_error(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Ok(None);
    }
    ensure_within_max_bytes(metadata.len(), max_bytes)?;
    read_bounded_bytes(file, max_bytes).map(Some)
}

/// Whether an open failed because the name holds nothing, or holds
/// something a read of a regular file cannot be aimed at.
fn is_non_regular_open_error(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::IsADirectory)
        || matches!(
            error.raw_os_error(),
            Some(libc::ELOOP | libc::EMLINK | libc::EISDIR | libc::ENXIO | libc::EOPNOTSUPP),
        )
}

/// Remove the regular file `name` inside `parent`, reporting whether one
/// was there to remove.
pub(super) fn unlink_file_at(parent: &File, name: &OsStr) -> io::Result<bool> {
    if stat_file_at(parent, name)?.is_none() {
        return Ok(false);
    }
    match pnpm_ffi_fs::unlink_at(parent, name) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Remove the directory `name` inside `parent` when it is empty, reporting
/// whether it went. A directory with entries in it is left alone.
pub(super) fn unlink_empty_dir_at(parent: &File, name: &OsStr) -> io::Result<bool> {
    match pnpm_ffi_fs::open_directory_at(parent, name) {
        Ok(dir) => drop(dir),
        Err(error) if is_not_a_real_dir(&error) => return Ok(false),
        Err(error) => return Err(error),
    }
    match pnpm_ffi_fs::remove_directory_at(parent, name) {
        Ok(()) => Ok(true),
        Err(error) if is_not_empty(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether a directory removal failed because the directory is gone or
/// still holds entries.
fn is_not_empty(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty)
        || matches!(error.raw_os_error(), Some(libc::ENOTEMPTY | libc::EEXIST))
}
