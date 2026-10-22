//! Windows process handles, for ending and awaiting a child.
//!
//! A handle pins the process object, so the id it was opened for cannot
//! come to name a different, recycled process. That is why pnpm opens one
//! at spawn time and terminates through it rather than looking the id up
//! again later.

use std::{
    os::windows::io::{
        AsHandle as _, AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle,
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, WAIT_TIMEOUT},
    System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
        WaitForSingleObject,
    },
};

/// A handle that can end and await one process.
///
/// `OwnedHandle` carries it, so it closes exactly once and crosses threads
/// without anything further being claimed about it.
pub struct ProcessHandle(OwnedHandle);

/// Whether a wait ended because the process did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// The process has exited.
    Exited,
    /// The wait ran out with the process still running.
    StillRunning,
}

impl ProcessHandle {
    /// Open a handle that can end and await the process `pid` names.
    ///
    /// Open it while the process is provably alive, such as right after a
    /// spawn while the spawner still holds its own handle: only then is the
    /// id certain to name the intended process. `None` when the process is
    /// gone or the access is refused, which leaves the caller unable to end
    /// it through a handle.
    #[must_use]
    pub fn open(pid: u32) -> Option<Self> {
        // SAFETY: the access mask and the id are taken by value, and a
        // handle the call returns is this call's to own. A failure is
        // reported as a null handle rather than written anywhere.
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: the handle is non-null and newly opened, so no other
        // owner exists to close it.
        Some(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    /// A handle of the caller's own to the same process.
    ///
    /// For passing a handle to a thread without the original's owner having
    /// to outlive it.
    #[must_use]
    pub fn duplicate(&self) -> Option<Self> {
        Self::duplicate_of(self.0.as_handle())
    }

    /// A handle of the caller's own to the process `source` names.
    #[must_use]
    pub fn duplicate_of(source: BorrowedHandle<'_>) -> Option<Self> {
        let mut duplicate = std::ptr::null_mut();
        // SAFETY: the source handle is open for the call, kept so by the
        // borrow, `GetCurrentProcess` returns the always-valid
        // current-process pseudo handle, and the output is a fully
        // initialized stack local that outlives the call.
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                source.as_raw_handle(),
                GetCurrentProcess(),
                &raw mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated == 0 {
            return None;
        }
        // SAFETY: the call reported success, so it wrote a handle that is
        // this call's to own.
        Some(Self(unsafe { OwnedHandle::from_raw_handle(duplicate) }))
    }

    /// End the process, reporting `exit_code` as its exit status.
    ///
    /// A process that has already exited is unaffected.
    pub fn terminate(&self, exit_code: u32) {
        // SAFETY: the handle is owned by `self`, open for the call, and was
        // opened with the terminate access this needs.
        unsafe {
            TerminateProcess(self.0.as_raw_handle(), exit_code);
        }
    }

    /// Wait up to `timeout` for the process to exit.
    #[must_use]
    pub fn wait(&self, timeout: Duration) -> Wait {
        let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle is owned by `self`, open for the call, and was
        // opened with the synchronize access this needs. The timeout is
        // taken by value.
        let waited = unsafe { WaitForSingleObject(self.0.as_raw_handle(), milliseconds) };
        if waited == WAIT_TIMEOUT { Wait::StillRunning } else { Wait::Exited }
    }
}
