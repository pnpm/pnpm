use std::{fs, io, path::Path};

/// Remove a regular file or directory that's occupying a symlink or file
/// slot. Tries `remove_dir_all` first; if the target isn't a
/// directory, falls back to `remove_file`.
fn remove_occupant(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => fs::remove_file(path),
    }
}

/// `fs::rename` that overwrites the destination when it exists.
/// Follows the
/// [`rename-overwrite`](https://github.com/zkochan/packages/tree/e65701a6ae/rename-overwrite)
/// package's approach: if the rename fails because the destination
/// is occupied (`AlreadyExists` for files, `DirectoryNotEmpty` for
/// dirs, `PermissionDenied` on Windows when something holds a handle
/// to the dest), remove the destination and retry once.
pub fn rename_overwrite(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(error) => {
            let occupied = matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists
                    | io::ErrorKind::DirectoryNotEmpty
                    | io::ErrorKind::PermissionDenied,
            );
            if !occupied {
                return Err(error);
            }
            remove_occupant(dst)?;
            fs::rename(src, dst)
        }
    }
}
