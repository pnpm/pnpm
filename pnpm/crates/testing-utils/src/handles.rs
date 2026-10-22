//! The process's standard handles, for a test that has to arrange how a
//! child inherits them.

use windows_sys::Win32::{
    Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation},
    System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    },
};

/// Mark this process's standard handles non-inheritable.
///
/// For a test that spawns a detached child and then asserts nothing it
/// leaves behind holds the test's own pipes open. Best-effort: a handle
/// that cannot be changed is left as it was.
pub fn stop_inheriting_standard_handles() {
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let Some(handle) = standard_handle(which) else { continue };
        // SAFETY: the handle came from `GetStdHandle` and was checked
        // against both values that are not a handle, and
        // `SetHandleInformation` changes a flag on it without taking
        // ownership, so nothing here closes a handle the process still
        // uses.
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

fn standard_handle(which: STD_HANDLE) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    // SAFETY: `GetStdHandle` takes one of the three documented stream
    // selectors by value and returns a handle the process already owns,
    // borrowing rather than creating it.
    let handle = unsafe { GetStdHandle(which) };
    (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(handle)
}
