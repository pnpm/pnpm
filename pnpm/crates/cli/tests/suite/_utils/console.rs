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

    /// Close the console's window, as a user closing the terminal does: the
    /// console then delivers `CTRL_CLOSE_EVENT` to every process attached to
    /// it and ends them. This process detaches first, so the close does not
    /// end the test too.
    pub fn close(self) {
        use windows_sys::Win32::{
            System::Console::GetConsoleWindow,
            UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
        };

        // SAFETY: plain FFI call with no arguments.
        let window = unsafe { GetConsoleWindow() };
        assert!(!window.is_null(), "the private console has a window");
        drop(self);
        // SAFETY: the window is the console's, which stays alive while the
        // processes still attached to it hold it open, and posting to a
        // window that has closed meanwhile only fails.
        let posted = unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        assert_ne!(posted, 0, "PostMessageW: {}", std::io::Error::last_os_error());
    }
}

/// Detaching leaves the handler in place. The console delivers a control
/// event to each attached process on a thread of its own, in no set order,
/// so a process the event ended can be gone before this one has received
/// it. Removing the handler then would let that late event end the test.
/// Once detached, this process has no console, so the handler sees nothing
/// further.
impl Drop for PrivateConsole {
    fn drop(&mut self) {
        // SAFETY: plain FFI call.
        unsafe {
            FreeConsole();
        }
    }
}

/// Reports every console control event as handled.
unsafe extern "system" fn survive_ctrl_c(_event: u32) -> windows_sys::core::BOOL {
    1
}
