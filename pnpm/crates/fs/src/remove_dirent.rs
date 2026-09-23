use crate::retry::retry_transient_removal_locks;
use std::{fs, io, path::Path};

/// Remove whatever occupies `path` without following links: a regular
/// file (or file-shaped symlink) is unlinked, a real directory is
/// removed recursively, and a directory-shaped link — a symlink to a
/// directory, or a junction on Windows — is unlinked without touching
/// its target. Dangling links are removed too.
///
/// The naive `is_dir()` dispatch to `remove_dir_all` / `remove_file`
/// is wrong on Windows twice over: following the link makes a dangling
/// link report as a non-directory, and even [`fs::symlink_metadata`]'s
/// [`fs::FileType::is_dir`] is `false` for a name-surrogate reparse
/// point. Either way the link is routed to `DeleteFileW`, which fails
/// on directory-shaped entries with `ERROR_ACCESS_DENIED` (os error 5);
/// they need the `RemoveDirectoryW` that [`crate::remove_symlink_dir`]
/// issues.
///
/// On Windows, a directory tree or file removal retries transient file
/// locks for up to a minute, access denied included, so a file that an
/// editor, indexer, or running program holds open below `path` delays the
/// removal instead of failing it. `retry_transient_removal_locks` explains
/// why access denied is waited out here.
pub fn remove_dirent(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        return remove_with_retry(path, fs::remove_dir_all);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return crate::remove_symlink_dir(path);
        }
    }
    remove_with_retry(path, fs::remove_file)
}

fn remove_with_retry<'path>(
    path: &'path Path,
    remove: impl Fn(&'path Path) -> io::Result<()>,
) -> io::Result<()> {
    retry_transient_removal_locks(|| {
        let result = remove(path);
        #[cfg(all(windows, feature = "test"))]
        crate::test_support::notify_attempt(path, &result);
        result
    })
}

#[cfg(test)]
mod tests;
