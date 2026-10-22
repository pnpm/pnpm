//! The forwarding of a delivery to the handler pnpm replaced.
//!
//! # This file must hold exactly one test
//!
//! A pristine process is this test's condition, not a convenience. A file
//! under `tests/` is its own binary, so the test below gets a process where
//! the `Once` behind [`pnpm_fs::install_temp_file_cleanup`] is unspent. That
//! is what lets it install `earlier` *underneath* pnpm's handler, so pnpm's
//! handler records `earlier` as the disposition it replaced and has
//! something to forward to. The altered `SIGTERM` then dies with the
//! process.
//!
//! Anything else added here that tracks a temp file or installs the cleanup
//! spends that `Once` first. pnpm's handler is then already in place,
//! `earlier` goes on top of it instead of underneath, and `raise` reaches
//! `earlier` directly: every assertion below still passes while no
//! forwarding has run at all. A second test here does not break this one
//! loudly, it hollows it out.
//!
//! So the test asserts the disposition it starts from rather than trusting
//! it, and that assertion is the thing to read if it ever fails.

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
    // SAFETY: a null `act` makes this a read, and `oldact` points at a fully
    // initialized stack local that outlives the call.
    let standing = unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        assert_eq!(libc::sigaction(libc::SIGTERM, std::ptr::null(), &raw mut current), 0);
        current.sa_sigaction
    };
    assert_eq!(
        standing,
        libc::SIG_DFL,
        "SIGTERM already carries a handler, so the `Once` behind \
         install_temp_file_cleanup is spent and this test would assert \
         nothing: see the comment at the top of this file",
    );

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

// Do not add a test below this line. The comment at the top of the file says
// why: a second test in this binary spends the `Once` and leaves the test
// above passing without forwarding anything.
