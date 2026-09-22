use super::{
    ForceSymlinkOutcome,
    Path,
    force_symlink_inner,
    fs,
    io,
    is_transient_file_lock_error,
    remove_dir_all_with_retry,
    rename_with_retry,
    retry_transient_file_locks,
};

/// The one-shot recoveries [`force_symlink_inner`] has already spent on a
/// link. Each bounds a recursion that must not repeat a step which did not help
/// the first time.
#[derive(Default, Clone, Copy)]
pub(super) struct TriedOnce {
    /// See [`clear_symlink_occupant`] for what the second pass does instead.
    pub(super) rename: bool,
    pub(super) vanished: bool,
}

/// Put the requested symlink at `link` when whatever stands there could not be
/// read back as one. `initial_err` is the create's own failure.
///
/// `None` from [`clear_symlink_occupant`] means there was nothing to clear, so
/// `initial_err` reported a conflict with something that has since gone and the
/// create is worth reissuing. [`TriedOnce::vanished`] bounds that, leaving a
/// link this can neither create at nor find anything at to surface its error.
pub(super) fn replace_unreadable_occupant(
    target: &Path,
    link: &Path,
    tried: TriedOnce,
    create_symlink: fn(&Path, &Path) -> io::Result<()>,
    initial_err: io::Error,
) -> io::Result<ForceSymlinkOutcome> {
    let Some(warning) = clear_symlink_occupant(link, tried.rename)? else {
        if tried.vanished {
            return Err(initial_err);
        }
        return force_symlink_inner(
            target,
            link,
            TriedOnce { vanished: true, ..tried },
            create_symlink,
        );
    };
    let mut outcome =
        force_symlink_inner(target, link, TriedOnce { rename: true, ..tried }, create_symlink)?;
    outcome.warning = Some(warning);
    Ok(outcome)
}

/// Move whatever regular file or directory occupies the link path out of the
/// way, and describe what was done with it. `None` means the occupant was
/// already gone.
///
/// On the second attempt (`rename_tried`) this drops down to a plain unlink,
/// as a fallback for an intermittent macOS bug — see
/// [pnpm/pnpm#5909](https://github.com/pnpm/pnpm/issues/5909#issuecomment-1400066890).
pub(super) fn clear_symlink_occupant(
    link: &Path,
    rename_tried: bool,
) -> io::Result<Option<String>> {
    let parent = link.parent().unwrap_or_else(|| Path::new(""));
    let basename = link
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if rename_tried {
        remove_occupant(link)?;
        return Ok(Some(format!(
            "Symlink wanted name was occupied by directory or file. \
             Old entity removed: {parent:?}{sep}{basename}",
            sep = std::path::MAIN_SEPARATOR,
        )));
    }
    let ignore_name = format!(".ignored_{basename}");
    if let Err(rename_err) = rename_overwrite(link, &parent.join(&ignore_name)) {
        if rename_err.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(rename_err);
    }
    Ok(Some(format!(
        "Symlink wanted name was occupied by directory or file. \
         Old entity moved: {parent:?}{sep}{basename} => {ignore_name}",
        sep = std::path::MAIN_SEPARATOR,
    )))
}

/// Remove a regular file or directory that's occupying a symlink
/// slot, retrying transient Windows file locks. The `remove_file`
/// fallback is taken only on `NotADirectory`, so a directory that stays
/// locked through its retry budget fails without starting a second one.
pub(super) fn remove_occupant(path: &Path) -> io::Result<()> {
    match remove_dir_all_with_retry(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
            retry_transient_file_locks(|| fs::remove_file(path))
        }
        Err(error) => Err(error),
    }
}

/// `fs::rename` that overwrites the destination when it exists.
/// Follows the
/// [`rename-overwrite`](https://github.com/zkochan/packages/tree/e65701a6ae/rename-overwrite)
/// package's approach: if the rename fails because the destination
/// is occupied (`AlreadyExists` for files, `DirectoryNotEmpty` for
/// dirs, `PermissionDenied` on Windows when something holds a handle
/// to the dest), remove the destination and retry. A transient Windows
/// file lock on either side is treated the same way: the destination
/// is cleared once, then the rename itself is retried.
pub(super) fn rename_overwrite(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(error) => {
            if !rename_error_allows_destination_removal(&error) {
                return Err(error);
            }
            remove_occupant(dst)?;
            rename_with_retry(src, dst)
        }
    }
}

pub(super) fn rename_error_allows_destination_removal(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AlreadyExists
            | io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::PermissionDenied,
    ) || is_transient_file_lock_error(error)
}
