//! Changes to the Windows console and to the process's standard handles.
//!
//! Both entry points are best-effort: each handle they cannot change is
//! left as it was, because every caller's fallback is the behaviour it
//! already had. Neither reports which handles it reached, since nothing in
//! pnpm would do anything different with the answer.

use crate::Stream;
use windows_sys::Win32::{
    Foundation::{HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation},
    System::Console::{
        ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
        STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    },
};

/// Mark the process's own standard handles non-inheritable, so that a
/// child spawned with handle inheritance does not also receive them as
/// stray duplicates.
pub fn stop_inheriting_standard_handles() {
    for stream in [Stream::Input, Stream::Output, Stream::Error] {
        let Some(handle) = standard_handle(stream) else { continue };
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

/// Switch the console behind each output stream into virtual-terminal
/// mode, so that it interprets ANSI escape sequences rather than printing
/// them.
pub fn enable_virtual_terminal_processing() {
    for stream in [Stream::Output, Stream::Error] {
        let Some(handle) = standard_handle(stream) else { continue };
        let Some(mode) = console_mode(handle) else { continue };
        // `ENABLE_VIRTUAL_TERMINAL_PROCESSING` is documented as requiring
        // `ENABLE_PROCESSED_OUTPUT`, so both are set.
        // SAFETY: as in `stop_inheriting_standard_handles`, the handle is
        // a checked one from `GetStdHandle`, and `SetConsoleMode` changes
        // its mode without taking ownership.
        unsafe {
            SetConsoleMode(
                handle,
                mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            );
        }
    }
}

/// The console mode of `handle`, or `None` when the stream is redirected to
/// a file or a pipe and so has no console mode.
fn console_mode(handle: HANDLE) -> Option<u32> {
    let mut mode = 0;
    // SAFETY: the handle is a checked one from `GetStdHandle`, and the
    // pointer is to a fully initialized stack local that outlives the call.
    let queried = unsafe { GetConsoleMode(handle, &raw mut mode) };
    (queried != 0).then_some(mode)
}

/// The process's handle for `stream`, or `None` when it has none.
///
/// The handle is borrowed from the process and must not be closed, which
/// is why it does not leave this module.
fn standard_handle(stream: Stream) -> Option<HANDLE> {
    let which: STD_HANDLE = match stream {
        Stream::Input => STD_INPUT_HANDLE,
        Stream::Output => STD_OUTPUT_HANDLE,
        Stream::Error => STD_ERROR_HANDLE,
    };
    // SAFETY: `GetStdHandle` takes one of the three documented stream
    // selectors by value and returns a handle the process already owns,
    // borrowing rather than creating it.
    let handle = unsafe { GetStdHandle(which) };
    (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(handle)
}
