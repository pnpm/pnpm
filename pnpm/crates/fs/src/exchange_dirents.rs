use std::{io, path::Path};

/// Swap the dirents at `first` and `second` in one step, so that neither
/// path is ever absent and each names one of the two trees throughout.
///
/// Linux (`renameat2` with `RENAME_EXCHANGE`) and macOS (`renamex_np`
/// with `RENAME_SWAP`) provide this on their main filesystems. Elsewhere,
/// and on a filesystem without the operation, the error has kind
/// [`io::ErrorKind::Unsupported`], and the caller swaps with two renames.
pub fn exchange_dirents(first: &Path, second: &Path) -> io::Result<()> {
    exchange(first, second)
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn exchange(first: &Path, second: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let first = CString::new(first.as_os_str().as_bytes())?;
    let second = CString::new(second.as_os_str().as_bytes())?;
    // The raw syscall rather than the `renameat2` wrapper, which not every
    // C library this is built against exports.
    // SAFETY: both pointers come from `CString`s that outlive the call, the
    // syscall takes exactly these five arguments, and the flag is the one
    // documented for an atomic exchange.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            first.as_ptr(),
            libc::AT_FDCWD,
            second.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    // SAFETY: both pointers come from `CString`s that outlive the call, and
    // the flag is the one documented for an atomic swap.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(first.as_ptr(), second.as_ptr(), libc::RENAME_SWAP) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // A kernel or filesystem without the operation reports it as an invalid
    // argument or an unsupported flag.
    match error.raw_os_error() {
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP) => {
            Err(io::Error::new(io::ErrorKind::Unsupported, error))
        }
        _ => Err(error),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn exchange(_first: &Path, _second: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
mod tests;
