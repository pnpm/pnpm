use super::{PackageBinSource, Path, fs, io, relative_path};
use miette::{Context, IntoDiagnostic};

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
    fs::rename(&staged, link)
        .inspect_err(|_| {
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

pub(super) fn io_error_report(error: io::Error, context: String) -> miette::Report {
    Err::<(), _>(error)
        .into_diagnostic()
        .wrap_err(context)
        .unwrap_err()
}

pub(super) fn remove_dir_all_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(in super::super) fn hash_linked_packages(
    packages: &[PackageBinSource],
    install_dir: &Path,
    hash_link: &Path,
) -> Vec<PackageBinSource> {
    packages
        .iter()
        .map(|package| match package.location.strip_prefix(install_dir) {
            Ok(relative) => PackageBinSource::new(
                hash_link.join(relative),
                std::sync::Arc::clone(&package.manifest),
            ),
            Err(_) => package.clone(),
        })
        .collect()
}
