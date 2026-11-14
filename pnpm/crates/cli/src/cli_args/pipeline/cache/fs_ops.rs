use std::{fs, io, path::Path};

/// Remove an existing output file, clearing the Windows read-only attribute
/// first when present, and refusing directories.
pub(super) fn remove_output_file(target: &Path) -> io::Result<()> {
    if !prepare_existing_output_file(target)? {
        return Ok(());
    }
    pnpm_fs::remove_file_with_retry(target)
}

/// Stage `source` into a sibling temporary file in `target`'s parent
/// directory, then atomically replace `target`.
///
/// Staging first keeps an existing valid output intact if reading or copying
/// `source` fails, while renaming a fresh file over `target` avoids `EACCES`
/// on `0444` files and never mutates hard-linked outputs in place.
pub(super) fn replace_output_file(source: &Path, target: &Path) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(target)
        && metadata.is_dir()
    {
        return Err(directory_output_error(target));
    }
    let parent = target
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let permissions = pnpm_fs::copy_permissions(source)?;
    let staged = tempfile::Builder::new()
        .make_in(parent, |temp_path| {
            pnpm_fs::copy_file_exclusive(source, temp_path, &permissions, |_| Ok(()))
        })?
        .into_temp_path();
    let _ = prepare_existing_output_file(target)?;
    pnpm_fs::rename_with_retry(&staged, target)?;
    let _ = staged.keep();
    Ok(())
}

fn prepare_existing_output_file(target: &Path) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() {
        return Err(directory_output_error(target));
    }
    #[cfg(windows)]
    if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "Windows Permissions holds only FILE_ATTRIBUTE_READONLY, cleared immediately before unlinking or rename replacement"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(target, permissions)?;
    }
    Ok(true)
}

fn directory_output_error(target: &Path) -> io::Error {
    let target_display = target.display();
    io::Error::other(format!("output path is a directory: {target_display}"))
}
