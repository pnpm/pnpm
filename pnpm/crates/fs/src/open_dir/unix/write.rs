//! Writing a file into a directory held open, so that a reader sees either
//! the old contents or the new ones and never a half-written file.

use super::is_symlink_at;
use crate::open_dir::{ChildWrite, unique_child_tmp_name};
use pnpm_ffi_fs::FileMode;
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, ErrorKind, Write as _},
};

/// How many names to try before giving up on finding a free one.
const MAX_TEMP_ATTEMPTS: usize = 16;

/// The mode a temporary file is created at, which the umask narrows.
const TEMP_MODE: FileMode = FileMode::from_mode(0o666);

/// Write `bytes` to `dest_name` inside `parent` through a temporary file.
///
/// The temporary file is written and flushed before it is given its final
/// name, so a concurrent reader of `dest_name` never sees a partial write.
/// `ChildWrite::CreateNew` publishes with a link, which fails when the name
/// is taken; `ChildWrite::Replace` renames over whatever is there, having
/// first refused a symlink, which a rename would otherwise replace.
pub(in crate::open_dir) fn write_file_at(
    parent: &File,
    base_name: &OsStr,
    dest_name: &OsStr,
    bytes: &[u8],
    mode: ChildWrite,
) -> io::Result<()> {
    if matches!(mode, ChildWrite::Replace) {
        refuse_symlink(parent, dest_name)?;
    }
    let (file, tmp_name) = create_temp_at(parent, base_name)?;
    if let Err(error) = write_and_sync(file, bytes) {
        discard(parent, &tmp_name);
        return Err(error);
    }
    let published = match mode {
        ChildWrite::CreateNew => pnpm_ffi_fs::link_at(parent, &tmp_name, dest_name),
        ChildWrite::Replace => replace_at(parent, &tmp_name, dest_name),
    };
    // A link leaves the temporary name in place on success, so it always
    // goes; a rename consumes it, so it only goes when the rename did not
    // happen.
    if matches!(mode, ChildWrite::CreateNew) || published.is_err() {
        discard(parent, &tmp_name);
    }
    published
}

/// Refuse to write through a symlink at `name`, which would hand the write
/// to whatever the link points at.
fn refuse_symlink(parent: &File, name: &OsStr) -> io::Result<()> {
    if is_symlink_at(parent, name)? {
        return Err(io::Error::new(ErrorKind::InvalidInput, "refusing to replace symlink child"));
    }
    Ok(())
}

fn write_and_sync(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    crate::sync_file(&file)
}

/// Create a file under a name nothing else holds, reporting it and the name
/// it took.
///
/// The create refuses an existing name rather than opening it, so two
/// writers racing for the same name cannot share a temporary file. A name
/// already taken is retried; anything else is the caller's error.
fn create_temp_at(parent: &File, base_name: &OsStr) -> io::Result<(File, OsString)> {
    let mut taken = None;
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let tmp_name = unique_child_tmp_name(base_name);
        match pnpm_ffi_fs::create_file_at(parent, &tmp_name, TEMP_MODE) {
            Ok(file) => return Ok((file, tmp_name)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => taken = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(taken.unwrap_or_else(|| {
        io::Error::new(ErrorKind::AlreadyExists, "temporary child collided")
    }))
}

/// Rename `tmp_name` onto `dest_name`, refusing a symlink that appeared
/// since the caller last looked.
fn replace_at(parent: &File, tmp_name: &OsStr, dest_name: &OsStr) -> io::Result<()> {
    refuse_symlink(parent, dest_name)?;
    pnpm_ffi_fs::rename_at(parent, tmp_name, dest_name)
}

/// Remove a temporary file whose write or publish did not finish. Nothing
/// can be done about a failure here, and the caller's own error is the one
/// worth reporting.
fn discard(parent: &File, name: &OsStr) {
    drop(pnpm_ffi_fs::unlink_at(parent, name));
}
