use std::{fs::File, io};

/// Flushes `file`'s data and metadata to the storage device, as
/// [`File::sync_all`] does.
///
/// On Apple platforms `sync_all` is `fcntl(F_FULLFSYNC)`, which some file
/// systems reject, such as virtiofs and SMB shares. There this falls back to
/// `fsync`, as `SQLite` and Node.js do.
pub fn sync_file(file: &File) -> io::Result<()> {
    let result = file.sync_all();
    #[cfg(target_vendor = "apple")]
    if let Err(error) = &result
        && is_full_fsync_unsupported(error)
    {
        return fsync(file);
    }
    result
}

#[cfg(target_vendor = "apple")]
fn is_full_fsync_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENOTTY | libc::ENOTSUP | libc::EOPNOTSUPP | libc::EINVAL),
    )
}

#[cfg(target_vendor = "apple")]
fn fsync(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;
    // SAFETY: `file` keeps the descriptor open for the whole call.
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests;
