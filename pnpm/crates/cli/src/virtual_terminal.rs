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
    pnpm_ffi_terminal::enable_virtual_terminal_processing();
}

#[cfg(not(windows))]
pub fn enable() {}
