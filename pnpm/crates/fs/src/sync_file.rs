use std::{fs::File, io};

/// Flushes `file`'s data and metadata to the storage device, as
/// [`File::sync_all`] does.
///
/// On Apple platforms `sync_all` is `fcntl(F_FULLFSYNC)`, which some file
/// systems reject, such as virtiofs and SMB shares. There this falls back to
/// `fsync`, as `SQLite` and Node.js do.
pub fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    return sync_with_fallback(|| file.sync_all(), || pnpm_ffi_fs::fsync(file));
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

#[cfg(test)]
mod tests;
