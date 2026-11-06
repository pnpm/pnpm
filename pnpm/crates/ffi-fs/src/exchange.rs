//! Swapping two directory entries in one step.

use std::{io, path::Path};

/// Swap the entries at `first` and `second` in one step, so that neither
/// path is ever absent and each names one of the two trees throughout.
///
/// Fails with [`io::ErrorKind::Unsupported`] on a platform, kernel, or
/// filesystem without the operation, which is the signal for a caller to
/// fall back to a pair of ordinary renames.
pub fn exchange_dirents(first: &Path, second: &Path) -> io::Result<()> {
    platform::exchange(first, second)
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
mod platform {
    use std::{ffi::CString, io, os::unix::ffi::OsStrExt as _, path::Path};

    pub(super) fn exchange(first: &Path, second: &Path) -> io::Result<()> {
        let first = path_name(first)?;
        let second = path_name(second)?;
        if swap(&first, &second) == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // A kernel or filesystem without the operation reports it as an
        // invalid argument or an unsupported flag, neither of which a
        // well-formed request can otherwise produce here.
        match error.raw_os_error() {
            Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP) => {
                Err(io::Error::new(io::ErrorKind::Unsupported, error))
            }
            _ => Err(error),
        }
    }

    /// The raw syscall rather than the `renameat2` wrapper, which not every
    /// C library this is built against exports.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn swap(first: &CString, second: &CString) -> libc::c_long {
        // SAFETY: both strings are NUL-terminated and borrowed for the
        // whole call. `SYS_renameat2` takes exactly these five arguments,
        // of these types: two directory descriptors with the two paths they
        // are resolved against, and a flag word. `AT_FDCWD` resolves an
        // absolute path and is the documented stand-in for the working
        // directory otherwise. The call reads both paths and retains
        // neither pointer.
        unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                first.as_ptr(),
                libc::AT_FDCWD,
                second.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        }
    }

    #[cfg(target_os = "macos")]
    fn swap(first: &CString, second: &CString) -> libc::c_int {
        // SAFETY: both strings are NUL-terminated and borrowed for the
        // whole call, and `renamex_np` takes the flag word by value. The
        // call reads both paths and retains neither pointer.
        unsafe { libc::renamex_np(first.as_ptr(), second.as_ptr(), libc::RENAME_SWAP) }
    }

    fn path_name(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
mod platform {
    use std::{io, path::Path};

    pub(super) fn exchange(_first: &Path, _second: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
