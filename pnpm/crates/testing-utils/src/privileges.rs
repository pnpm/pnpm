//! Dropping a thread's privileges for the length of a test.
//!
//! An elevated test runner can bypass an ACL through its backup and
//! restore privileges, so a test that asserts an ACL is enforced has to
//! give those up first. Impersonation confines the change to the calling
//! thread, which is what keeps it from reaching the rest of the suite.

use std::{
    io,
    os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Security::{
        AdjustTokenPrivileges, ImpersonateSelf, RevertToSelf, SecurityImpersonation,
        TOKEN_ADJUST_PRIVILEGES,
    },
    System::Threading::{GetCurrentThread, OpenThreadToken},
};

/// Run `operation` on this thread with every privilege disabled.
///
/// Panics rather than reporting a failure: a test that cannot drop its
/// privileges would silently assert nothing.
pub fn without_thread_privileges(operation: impl FnOnce()) {
    let _revert = Impersonation::begin();
    let token = thread_token();
    assert_ne!(
        // SAFETY: the token was opened with `TOKEN_ADJUST_PRIVILEGES` and
        // is owned for the call. Disabling every privilege is the one form
        // that permits a null privilege set, a zero buffer length, and null
        // previous-state and returned-length pointers.
        unsafe {
            AdjustTokenPrivileges(token.as_raw_handle(), 1, null(), 0, null_mut(), null_mut())
        },
        0,
        "{}",
        io::Error::last_os_error(),
    );
    operation();
}

/// An impersonation of this thread's own token, reverted when dropped.
struct Impersonation;

impl Impersonation {
    fn begin() -> Self {
        assert_ne!(
            // SAFETY: `SecurityImpersonation` is one of the documented
            // levels, taken by value, and the call takes no pointer.
            unsafe { ImpersonateSelf(SecurityImpersonation) },
            0,
            "{}",
            io::Error::last_os_error(),
        );
        Self
    }
}

impl Drop for Impersonation {
    fn drop(&mut self) {
        // SAFETY: the guard never leaves the thread that impersonated, so
        // this reverts the impersonation `begin` started rather than
        // another thread's.
        assert_ne!(unsafe { RevertToSelf() }, 0, "{}", io::Error::last_os_error());
    }
}

fn thread_token() -> OwnedHandle {
    let mut token = null_mut();
    assert_ne!(
        // SAFETY: `GetCurrentThread` returns the current-thread pseudo
        // handle, which is always valid and must not be closed, and the
        // output is a fully initialized stack local that outlives the call.
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_ADJUST_PRIVILEGES, 1, &raw mut token) },
        0,
        "{}",
        io::Error::last_os_error(),
    );
    // SAFETY: the call reported success, so it wrote a handle that is this
    // call's to own.
    unsafe { OwnedHandle::from_raw_handle(token) }
}
