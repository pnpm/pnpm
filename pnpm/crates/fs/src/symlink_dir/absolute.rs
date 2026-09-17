use super::{ForceSymlinkOutcome, force_symlink_inner, to_native_separators};
use std::{io, path::Path};

/// [`force_symlink_dir`](super::force_symlink_dir) with the link holding
/// `target` as given rather than a path relative to the link, for a link
/// that must keep pointing at `target` when the directory holding the
/// link moves, such as a project's link to an environment in the store.
pub fn force_absolute_symlink_dir(target: &Path, link: &Path) -> io::Result<ForceSymlinkOutcome> {
    let target = to_native_separators(target);
    let link = to_native_separators(link);
    #[cfg(windows)]
    return force_symlink_inner(&target, &link, false, super::windows::create_absolute);
    #[cfg(not(windows))]
    force_symlink_inner(&target, &link, false, absolute_symlink_dir)
}

#[cfg(unix)]
fn absolute_symlink_dir(original: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}
