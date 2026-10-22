use std::{fs::File, io};

/// Flushes `file`'s data and metadata to the storage device, as
/// [`File::sync_all`] does.
///
/// On Apple platforms `sync_all` is `fcntl(F_FULLFSYNC)`, which some file
/// systems reject, such as virtiofs and SMB shares. There this falls back to
/// `fsync`, as `SQLite` and Node.js do.
pub fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    return sync_with_fallback(|| file.sync_all(), || fsync(file));
    #[cfg(not(target_vendor = "apple"))]
    file.sync_all()
}

/// Runs `full_sync`, and `fsync` only if the file system does not support it.
#[cfg(target_vendor = "apple")]
fn sync_with_fallback(
    full_sync: impl FnOnce() -> io::Result<()>,
    fsync: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    match full_sync() {
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ENOTTY | libc::ENOTSUP | libc::EOPNOTSUPP | libc::EINVAL),
            ) =>
        {
            fsync()
        }
        result => result,
    }
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
