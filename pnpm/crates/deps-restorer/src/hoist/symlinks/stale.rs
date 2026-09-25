use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

/// Read the existing symlink at `dest` and decide whether it should
/// be replaced. If it already points at `dep_dir`, leave it untouched.
/// If it points inside `package_store_dir` or `internal_pnpm_dir`
/// (a pnpm-internal symlink — e.g., a stale link from a prior non-GVS
/// install), remove it and create a new symlink to `dep_dir`. External
/// symlinks (and non-symlink occupants) are left in place.
///
/// The already-correct fast path skips the unlink + recreate churn (and
/// the transient missing-link window it opens) on warm reinstalls, the
/// same way [`pnpm_fs::force_symlink_dir`] does — see its
/// `existing_symlink_up_to_date` helper.
pub(in crate::hoist) fn update_stale_hoist_symlink(
    dep_dir: &Path,
    dest: &Path,
    package_store_dir: &Path,
    internal_pnpm_dir: &Path,
) -> Result<(), crate::SymlinkPackageError> {
    let Ok(existing) = read_hoist_symlink(dest) else {
        return Ok(());
    };
    if pnpm_fs::lexical_normalize(&existing) == pnpm_fs::lexical_normalize(dep_dir) {
        return Ok(());
    }
    if !pnpm_fs::is_subdir(package_store_dir, &existing)
        && !pnpm_fs::is_subdir(internal_pnpm_dir, &existing)
    {
        return Ok(());
    }
    replace_stale_hoist_symlink(dep_dir, dest)
        .map_err(|error| crate::SymlinkPackageError::SymlinkDir {
            symlink_target: dep_dir.to_path_buf(),
            symlink_path: dest.to_path_buf(),
            error,
        })
}

fn replace_stale_hoist_symlink(dep_dir: &Path, dest: &Path) -> io::Result<()> {
    match pnpm_fs::remove_symlink_dir(dest) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    create_hoist_symlink(dep_dir, dest)
}

fn create_hoist_symlink(dep_dir: &Path, dest: &Path) -> io::Result<()> {
    loop {
        let error = match pnpm_fs::symlink_dir(dep_dir, dest) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => error,
            result => return result,
        };
        match read_hoist_symlink(dest) {
            Ok(existing)
                if pnpm_fs::lexical_normalize(&existing) == pnpm_fs::lexical_normalize(dep_dir) =>
            {
                return Ok(());
            }
            Err(read_error) if read_error.kind() == io::ErrorKind::NotFound => {}
            Err(read_error) if read_error.kind() == io::ErrorKind::InvalidInput => {
                // macOS can report EINVAL when a concurrent unlink interrupts read_link.
                match pnpm_fs::symlink_metadata_with_retry(dest) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {}
                    Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => {}
                    _ => return Err(error),
                }
            }
            _ => return Err(error),
        }
    }
}

fn read_hoist_symlink(dest: &Path) -> io::Result<PathBuf> {
    let existing = pnpm_fs::read_symlink_dir(dest)?;
    Ok(if existing.is_relative() {
        dest.parent()
            .unwrap_or_else(|| Path::new(""))
            .join(existing)
    } else {
        existing
    })
}
