//! A package's symlink to something inside the package, as a copy of the
//! package recreates it.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// `link`, a link target read from `link_path`, with an absolute one made
/// relative to the link's directory. Both sides are canonicalized first:
/// a directory walk lists links under the canonical package root, while the
/// link text may reach the same place through another path.
#[must_use]
pub fn relative_link_target(link_path: &Path, link: PathBuf) -> PathBuf {
    let (true, Some(parent)) = (link.is_absolute(), link_path.parent()) else {
        return link;
    };
    let parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let link = fs::canonicalize(&link).unwrap_or(link);
    pathdiff::diff_paths(&link, &parent).unwrap_or(link)
}

/// Create a file link at `link` holding `contents`. A file symlink has no
/// junction to fall back to, so a process Windows refuses symlinks gets a
/// copy of `original`, the file the link points at.
pub fn symlink_file_or_copy(contents: &Path, original: &Path, link: &Path) -> io::Result<()> {
    let result = crate::create_symlink(contents, link, false);
    #[cfg(windows)]
    {
        if let Err(error) = &result
            && error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD)
        {
            return fs::copy(original, link).map(drop);
        }
    }
    #[cfg(not(windows))]
    let _ = original;
    result
}

#[cfg(windows)]
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
