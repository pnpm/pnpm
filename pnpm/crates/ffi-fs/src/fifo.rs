//! Creating a FIFO.
//!
//! pnpm never creates one in earnest. The tests do, to pin that the paths
//! which must refuse anything but a regular file refuse one: a FIFO is the
//! entry whose `open` blocks on a writer, so it is what a path opened
//! without `O_NONBLOCK` hangs on.

use std::{ffi::CString, io, os::unix::ffi::OsStrExt as _, path::Path};

/// Create a FIFO at `path` with `mode`, before the umask is applied to it.
pub fn create_fifo(path: &Path, mode: u32) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mode = libc::mode_t::try_from(mode)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mode does not fit mode_t"))?;
    // SAFETY: `path` is NUL-terminated and borrowed for the whole call, and
    // `mkfifo` takes the mode by value. The call reads the path and retains
    // no pointer.
    let created = unsafe { libc::mkfifo(path.as_ptr(), mode) };
    if created == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}
