//! A console of the test process's own, for the tests that send one a
//! control event. The console delivers `Ctrl+C` to every process attached
//! to it, so the console running the test suite must not be the one.
#![cfg(windows)]

use windows_sys::Win32::System::Console::{
    AllocConsole, CTRL_C_EVENT, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler,
};

/// A private console attached to this test process for as long as the
/// value lives. Each test runs in its own process under nextest, so the
/// swap affects no other test.
///
/// Ctrl+C handling is switched back on, because the test runner may have
/// started this process with it off, and children inherit that. A handler
/// then keeps this process itself alive through a Ctrl+C sent to the whole
/// console.
pub struct PrivateConsole;

impl PrivateConsole {
    pub fn attach() -> Self {
        // SAFETY: plain FFI calls. Detaching fails harmlessly when the process
        // has no console to begin with. `survive_ctrl_c` is a `'static` function
        // that touches no state, so it is sound to call on any thread.
        unsafe {
            FreeConsole();
            assert_ne!(AllocConsole(), 0, "AllocConsole: {}", std::io::Error::last_os_error());
            SetConsoleCtrlHandler(None, 0);
            SetConsoleCtrlHandler(Some(survive_ctrl_c), 1);
        }
        PrivateConsole
    }

    /// `Ctrl+C` at this console, which reaches every process attached to
    /// it, as a keypress does. This process ignores it (see
    /// [`PrivateConsole`]).
    #[expect(clippy::unused_self, reason = "the event goes to the console this value attached")]
    pub fn press_ctrl_c(&self) {
        // SAFETY: plain FFI call with no pointer arguments. Group 0 is the
        // whole private console.
        let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) };
        assert_ne!(sent, 0, "GenerateConsoleCtrlEvent: {}", std::io::Error::last_os_error());
    }
}

impl Drop for PrivateConsole {
    fn drop(&mut self) {
        // SAFETY: plain FFI calls; the handler removed is the one `attach` added.
        unsafe {
            SetConsoleCtrlHandler(Some(survive_ctrl_c), 0);
            FreeConsole();
        }
    }
}

/// Reports every console control event as handled.
unsafe extern "system" fn survive_ctrl_c(_event: u32) -> windows_sys::core::BOOL {
    1
}
