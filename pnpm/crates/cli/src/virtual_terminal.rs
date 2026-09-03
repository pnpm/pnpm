//! Turn on the Windows console's ANSI escape handling.
//!
//! `supports-color`, the backend behind `owo-colors`, treats every Windows
//! 10 console as ANSI-capable and leaves it to the application to switch
//! the console into virtual-terminal mode. Until that happens `cmd.exe`
//! prints each escape sequence literally, so `←[32m` shows up where green
//! text belongs. Unix terminals interpret the sequences without any setup,
//! so [`enable`] is a no-op there.

#[cfg(windows)]
pub fn enable() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
        STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    };

    // SAFETY: these are standard Win32 console calls. `GetStdHandle` returns
    // a borrowed handle that must not be closed, and the only pointer passed
    // is a stack local that outlives the call.
    unsafe {
        for stream in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(stream);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut mode = 0;
            // Fails when the stream is redirected to a file or a pipe, which
            // has no console mode to change.
            if GetConsoleMode(handle, &mut mode) == 0 {
                continue;
            }
            // `ENABLE_VIRTUAL_TERMINAL_PROCESSING` is documented as requiring
            // `ENABLE_PROCESSED_OUTPUT`, so both are set. A console that
            // refuses the new mode keeps the old one and prints the escape
            // sequences it did before, which is why the result is ignored.
            SetConsoleMode(
                handle,
                mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            );
        }
    }
}

#[cfg(not(windows))]
pub fn enable() {}
