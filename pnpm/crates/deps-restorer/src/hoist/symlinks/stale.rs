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
    let symlink_error = |error| crate::SymlinkPackageError::SymlinkDir {
        symlink_target: dep_dir.to_path_buf(),
        symlink_path: dest.to_path_buf(),
        error,
    };
    let existing = match read_hoist_symlink(dest) {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return create_hoist_symlink(dep_dir, dest).map_err(symlink_error);
        }
        Err(error) if is_non_link_read_error(&error) => return Ok(()),
        Err(error) => return Err(symlink_error(error)),
    };
    if pnpm_fs::lexical_normalize(&existing) == pnpm_fs::lexical_normalize(dep_dir) {
        return Ok(());
    }
    if !pnpm_fs::is_subdir(package_store_dir, &existing)
        && !pnpm_fs::is_subdir(internal_pnpm_dir, &existing)
    {
        return Ok(());
    }
    replace_stale_hoist_symlink(dep_dir, dest).map_err(symlink_error)
}

fn is_non_link_read_error(error: &io::Error) -> bool {
    const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;
    error.kind() == io::ErrorKind::InvalidInput
        || (cfg!(windows) && error.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT))
}

fn replace_stale_hoist_symlink(dep_dir: &Path, dest: &Path) -> io::Result<()> {
    match pnpm_fs::remove_symlink_dir(dest) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    create_hoist_symlink(dep_dir, dest)
}

fn create_hoist_symlink(dep_dir: &Path, dest: &Path) -> io::Result<()> {
    let mut retries = 0;
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
            Err(read_error) if should_retry_hoist_link_read(dest, &read_error) && retries < 100 => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(existing) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; link at {} points to {}, expected {}",
                        dest.display(),
                        existing.display(),
                        dep_dir.display(),
                    ),
                ));
            }
            Err(read_error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{error}; failed to inspect link at {}: {read_error}", dest.display()),
                ));
            }
        }
    }
}

fn should_retry_hoist_link_read(dest: &Path, error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::NotFound {
        return true;
    }
    if !is_non_link_read_error(error) {
        return false;
    }
    // A concurrent unlink can make a link read report that the entry is not a link.
    match pnpm_fs::symlink_metadata_with_retry(dest) {
        Ok(metadata) => is_link_metadata(&metadata) || may_be_junction_in_creation(dest, &metadata),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// A junction is created as an empty directory that gets its reparse point
/// afterwards, so a concurrent hoist can find an empty plain directory in its
/// place for a moment.
fn may_be_junction_in_creation(dest: &Path, metadata: &std::fs::Metadata) -> bool {
    cfg!(windows)
        && metadata.is_dir()
        && std::fs::read_dir(dest).is_ok_and(|mut entries| entries.next().is_none())
}

fn is_link_metadata(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
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
