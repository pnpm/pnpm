use super::{Path, fs, io, relative_path};

/// Point `link` at `target`, replacing any existing link in a single step
/// so a concurrent command never observes it missing. Windows cannot
/// rename over an existing junction, so there the replacement falls back
/// to the non-atomic remove-and-recreate.
pub(super) fn swap_hash_link_atomically(target: &Path, link: &Path) -> io::Result<()> {
    if cfg!(windows) {
        return pnpm_fs::force_symlink_dir(target, link).map(|_| ());
    }
    let Some(parent) = link.parent() else {
        return pnpm_fs::force_symlink_dir(target, link).map(|_| ());
    };
    fs::create_dir_all(parent)?;
    let staged = link.with_extension(format!("{}.tmp", std::process::id()));
    match fs::remove_file(&staged) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    symlink_dir_entry(&relative_path(parent, target), &staged)?;
    fs::rename(&staged, link).inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })
}

#[cfg(unix)]
fn symlink_dir_entry(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink_dir_entry(target: &Path, link: &Path) -> io::Result<()> {
    pnpm_fs::force_symlink_dir(target, link).map(|_| ())
}
