//! Keep pnpm's own standard handles out of the processes it spawns.
//!
//! A process inherits its standard handles marked inheritable, and
//! `CreateProcess` with `bInheritHandles` passes every inheritable handle to
//! the child. Rust's `Stdio::inherit` gives the child an inheritable duplicate
//! as its standard handle, so the originals reach it as stray handles that
//! nothing in the child closes. A detached grandchild, such as the Nx daemon,
//! keeps them open, and a shell reading pnpm's redirected output waits until
//! that grandchild exits. [`disable`] does what Node.js does at startup and
//! marks the originals non-inheritable. On Unix the child's standard streams
//! are the same descriptors, so no stray copy exists and [`disable`] is a
//! no-op.

#[cfg(windows)]
pub fn disable() {
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    // SAFETY: `GetStdHandle` returns a borrowed handle that must not be
    // closed, and `SetHandleInformation` only changes its inherit flag.
    // A failure leaves the handle inheritable, as it was.
    unsafe {
        for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(stream);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

#[cfg(not(windows))]
pub fn disable() {}
