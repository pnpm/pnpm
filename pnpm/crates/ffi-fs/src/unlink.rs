//! Removing an entry from a context that may not allocate.

use std::ffi::CStr;

/// Remove the entry at `path`.
///
/// For a signal handler, which may not allocate:
/// [`std::fs::remove_file`] builds a C string from the path it is given and
/// so allocates, while `unlink` is async-signal-safe. The path is taken
/// already encoded, which is what moves that work out of the handler.
///
/// Best-effort, because the handler's caller has nothing it could do with
/// a failure: the process is about to end either way.
pub fn unlink_without_allocating(path: &CStr) {
    // SAFETY: `path` is NUL-terminated by its type and borrowed for the
    // whole call, which reads it and retains no pointer.
    unsafe {
        libc::unlink(path.as_ptr());
    }
}
