use std::io;

/// Whether `error` is the kernel refusing an operation because its two
/// paths are not on one filesystem.
///
/// EXDEV = "cross-device link not permitted". Linux / macOS / BSD all
/// use errno 18; Windows maps its equivalent `ERROR_NOT_SAME_DEVICE`
/// to raw OS error 17. pnpm detects this by checking
/// `err.message.startsWith('EXDEV: cross-device link not permitted')` —
/// we can be a little tighter by looking at the raw errno.
///
/// The `17` mapping must stay Windows-only: on Unix, raw 17 is
/// `EEXIST` (surfaces as [`io::ErrorKind::AlreadyExists`]), which for a
/// hardlink means a concurrent process created the target and for a
/// rename means the destination is already occupied. Reading either as
/// cross-device would overwrite that other content with a copy.
#[must_use]
pub fn is_cross_device(error: &io::Error) -> bool {
    #[cfg(unix)]
    return error.raw_os_error() == Some(18);
    #[cfg(windows)]
    return error.raw_os_error() == Some(17);
    #[cfg(not(any(unix, windows)))]
    return false;
}

#[cfg(test)]
mod tests;
