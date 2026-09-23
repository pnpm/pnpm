use crate::{remove_dir_all_with_retry, remove_file_with_retry};
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
/// Every removal retries transient Windows file locks with the policy of
/// [`crate::rename_with_retry`], because an editor or indexer that holds a
/// file open below `path` blocks the removal only for a moment.
pub fn remove_dirent(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        return remove_dir_all_with_retry(path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return crate::remove_symlink_dir(path);
        }
    }
    remove_file_with_retry(path)
}

#[cfg(test)]
mod tests;
