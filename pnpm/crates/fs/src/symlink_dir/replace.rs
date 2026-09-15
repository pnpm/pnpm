use super::{
    Path, fs, io, is_transient_file_lock_error, remove_dir_all_with_retry, rename_with_retry,
    retry_transient_file_locks,
};

/// Remove a regular file or directory that's occupying a symlink
/// slot, retrying transient Windows file locks. The `remove_file`
/// fallback is taken only on `NotADirectory`, so a directory that stays
/// locked through its retry budget fails without starting a second one.
pub(super) fn remove_occupant(path: &Path) -> io::Result<()> {
    match remove_dir_all_with_retry(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
            retry_transient_file_locks(|| fs::remove_file(path))
        }
        Err(error) => Err(error),
    }
}

/// `fs::rename` that overwrites the destination when it exists.
/// Follows the
/// [`rename-overwrite`](https://github.com/zkochan/packages/tree/e65701a6ae/rename-overwrite)
/// package's approach: if the rename fails because the destination
/// is occupied (`AlreadyExists` for files, `DirectoryNotEmpty` for
/// dirs, `PermissionDenied` on Windows when something holds a handle
/// to the dest), remove the destination and retry. A transient Windows
/// file lock on either side is treated the same way: the destination
/// is cleared once, then the rename itself is retried.
pub(super) fn rename_overwrite(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(error) => {
            if !rename_error_allows_destination_removal(&error) {
                return Err(error);
            }
            remove_occupant(dst)?;
            rename_with_retry(src, dst)
        }
    }
}

pub(super) fn rename_error_allows_destination_removal(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AlreadyExists
            | io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::PermissionDenied,
    ) || is_transient_file_lock_error(error)
}
