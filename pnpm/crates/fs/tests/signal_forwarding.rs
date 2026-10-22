//! The forwarding of a delivery to the handler pnpm replaced.
//!
//! One test in a file of its own, which is what gives it a process of its
//! own. It has to install its handler before the `Once` behind
//! [`pnpm_fs::install_temp_file_cleanup`] is spent, which a sibling that
//! tracked a temp file first would have spent already, and it leaves
//! `SIGTERM` pointing at pnpm's handler afterwards. A second test in this
//! file would take both of those away.

#![cfg(unix)]

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

static REACHED_WITH: AtomicI32 = AtomicI32::new(0);
static DETAILS_MATCHED: AtomicBool = AtomicBool::new(false);

extern "C" fn earlier(
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

/// Asserts on the `siginfo_t` the replaced handler received rather than only
/// on its being reached, because a wrong signature leaves those arguments
/// holding whatever the registers did.
#[test]
fn a_replaced_siginfo_handler_is_passed_the_signal_details() {
    // SAFETY: an all-zero `sigaction` is a valid one to build on, and the
    // pointer is to a fully initialized stack local that outlives the call.
    // A null `oldact` discards the disposition being replaced.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_flags = libc::SA_SIGINFO;
        action.sa_sigaction = earlier as *const () as usize;
        libc::sigemptyset(&raw mut action.sa_mask);
        assert_eq!(libc::sigaction(libc::SIGTERM, &raw const action, std::ptr::null_mut()), 0);
    }

    // pnpm's handler goes on top, so `earlier` is the disposition it records
    // and the one it passes the signal on to.
    pnpm_fs::install_temp_file_cleanup();

    // SAFETY: `raise` delivers to this thread, where the disposition is
    // pnpm's handler over `earlier`, so neither ends the process and the call
    // returns.
    assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);

    assert_eq!(
        REACHED_WITH.load(Ordering::Acquire),
        libc::SIGTERM,
        "the signal was not passed on to the handler pnpm replaced",
    );
    assert!(
        DETAILS_MATCHED.load(Ordering::Acquire),
        "the replaced handler was called without the delivery's `siginfo_t`",
    );
}
