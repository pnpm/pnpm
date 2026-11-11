//! Driving a signal delivery from a test.
//!
//! The foreign calls a test needs to stand in for a program that installed
//! its own handler before pnpm did. One signal and one recording handler per
//! process, which is what a caller gets by holding its test to a file of its
//! own; a second caller in the same process overwrites the first's record.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// What the handler a test installed was called with, once the handler that
/// replaced it forwarded the delivery on.
pub struct ForwardedDelivery {
    /// The signal number the replaced handler received.
    pub signal: i32,
    /// Whether the `siginfo_t` it received described that same signal, which
    /// a handler reached through the one-argument signature cannot report.
    pub details_matched: bool,
}

static REACHED_WITH: AtomicI32 = AtomicI32::new(0);
static DETAILS_MATCHED: AtomicBool = AtomicBool::new(false);

extern "C" fn record(
    signal: libc::c_int,
    siginfo: *mut libc::siginfo_t,
    _context: *mut libc::c_void,
) {
    REACHED_WITH.store(signal, Ordering::Release);
    // SAFETY: the kernel hands a handler installed with `SA_SIGINFO` a valid
    // `siginfo_t` for the delivery, and this reads one `c_int` field of it
    // without retaining the pointer.
    let matched = !siginfo.is_null() && unsafe { (*siginfo).si_signo } == signal;
    DETAILS_MATCHED.store(matched, Ordering::Release);
}

/// Install a handler for `signal` that asks for the delivery's details, run
/// `install` so the subject's own handler replaces it, then raise `signal`
/// and report what the replaced handler was called with.
///
/// `install` runs between the two so that the handler above is the
/// disposition the subject records and forwards to.
pub fn forwarded_delivery(signal: i32, install: impl FnOnce()) -> ForwardedDelivery {
    // SAFETY: an all-zero `sigaction` is a valid one to build on, and the
    // pointer is to a fully initialized stack local that outlives the call.
    // A null `oldact` discards the disposition being replaced.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_flags = libc::SA_SIGINFO;
        action.sa_sigaction = record as *const () as usize;
        libc::sigemptyset(&raw mut action.sa_mask);
        assert_eq!(libc::sigaction(signal, &raw const action, std::ptr::null_mut()), 0);
    }

    install();

    // SAFETY: `raise` delivers to this thread, where the disposition is the
    // subject's handler over the one above, so neither ends the process and
    // the call returns.
    assert_eq!(unsafe { libc::raise(signal) }, 0);

    ForwardedDelivery {
        signal: REACHED_WITH.load(Ordering::Acquire),
        details_matched: DETAILS_MATCHED.load(Ordering::Acquire),
    }
}
