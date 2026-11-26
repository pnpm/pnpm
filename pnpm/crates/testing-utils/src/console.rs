//! A console of the test process's own, for the tests that send one a
//! control event. The console delivers `Ctrl+C` to every process attached
//! to it, so the console running the test suite must not be the one.

use windows_sys::Win32::System::Console::{
    AllocConsole,
    CTRL_BREAK_EVENT,
    CTRL_C_EVENT,
    FreeConsole,
    GenerateConsoleCtrlEvent,
    SetConsoleCtrlHandler,
};

/// Which control event to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleEvent {
    /// What `Ctrl+C` raises.
    CtrlC,
    /// What `Ctrl+Break` raises, and the only event that can be aimed at
    /// one process group rather than the whole console.
    CtrlBreak,
}

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
    pub fn press_ctrl_c(&self) {
        self.send(ConsoleEvent::CtrlC, None);
    }

    /// Send `event` to the process group `group` leads, or to the whole
    /// console when `group` is `None`.
    ///
    /// Takes `&self` though it reads nothing from it: the event goes to the
    /// console this value attached.
    pub fn send(&self, event: ConsoleEvent, group: Option<u32>) {
        let event = match event {
            ConsoleEvent::CtrlC => CTRL_C_EVENT,
            ConsoleEvent::CtrlBreak => CTRL_BREAK_EVENT,
        };
        // SAFETY: both arguments are taken by value and the call takes no
        // pointer. Group 0 is the whole console, where this process ignores
        // the event (see `PrivateConsole`); any other group is one the
        // caller created for a child of its own.
        let sent = unsafe { GenerateConsoleCtrlEvent(event, group.unwrap_or(0)) };
        assert_ne!(sent, 0, "GenerateConsoleCtrlEvent: {}", std::io::Error::last_os_error());
    }

    /// Close the console's window, as a user closing the terminal does: the
    /// console then delivers `CTRL_CLOSE_EVENT` to every process attached to
    /// it and ends them. This process detaches first, so the close does not
    /// end the test too.
    pub fn close(self) {
        use windows_sys::Win32::{
            Foundation::{
                ERROR_SUCCESS,
                ERROR_TIMEOUT,
                SetLastError,
            },
            System::Console::GetConsoleWindow,
            UI::WindowsAndMessaging::{
                SMTO_ABORTIFHUNG,
                SendMessageTimeoutW,
                WM_CLOSE,
            },
        };

        // SAFETY: plain FFI call with no arguments.
        let window = unsafe { GetConsoleWindow() };
        assert!(!window.is_null(), "the private console has a window");
        drop(self);
        // SAFETY: plain FFI call that takes an integer. The send does not
        // set the last error on every failure, so a stale `ERROR_TIMEOUT`
        // must not reach the check below.
        unsafe { SetLastError(ERROR_SUCCESS) };
        // SAFETY: the handle came from `GetConsoleWindow` and is non-null.
        // The send takes no pointer but the result slot, which may be null.
        // A window that closed meanwhile makes the send fail, which is
        // harmless.
        // The send waits until the console host has processed the close, so
        // the caller observes its effect.
        let sent = unsafe {
            SendMessageTimeoutW(
                window,
                WM_CLOSE,
                0,
                0,
                SMTO_ABORTIFHUNG,
                30_000,
                std::ptr::null_mut(),
            )
        };
        if sent == 0 {
            let error = std::io::Error::last_os_error();
            assert_ne!(
                error.raw_os_error(),
                Some(ERROR_TIMEOUT.cast_signed()),
                "the console host did not process WM_CLOSE: {error}",
            );
        }
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
