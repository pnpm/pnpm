//! Holding a signal queued and undelivered while a test reads what a
//! subject makes of it.
//!
//! The foreign calls a test needs to stand a signal up as pending without
//! letting it land. A subject that reads the kernel's queue can then be
//! observed at a moment the process never reaches on its own: between the
//! kernel queueing a delivery and a handler running for it.

/// Queue `signal` for this thread with the thread blocking it, call
/// `observe` while the delivery sits on the queue, then take the delivery
/// back off and put this thread's signal mask as it was.
///
/// No handler runs for it, so the subject is read at the one moment the
/// delivery is queued and unhandled, and nothing is left behind for the
/// rest of the test. Aiming the signal at this thread rather than at the
/// process is what keeps it from reaching the test harness's other
/// threads, which do not block it.
pub fn while_pending<Observed>(signal: i32, observe: impl FnOnce() -> Observed) -> Observed {
    // SAFETY: an all-zero `sigset_t` is a valid one to build on, and every
    // pointer is to a fully initialized stack local that outlives its call.
    // The signal is aimed at this thread, which blocks it, so no handler
    // runs and no default action ends the process.
    let (blocked, previous) = unsafe {
        let mut blocked: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut blocked);
        libc::sigaddset(&raw mut blocked, signal);
        let mut previous: libc::sigset_t = std::mem::zeroed();
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, &raw const blocked, &raw mut previous),
            0,
            "blocking signal {signal} failed: {}",
            std::io::Error::last_os_error(),
        );
        assert_eq!(
            libc::pthread_kill(libc::pthread_self(), signal),
            0,
            "queueing signal {signal} failed: {}",
            std::io::Error::last_os_error(),
        );
        (blocked, previous)
    };

    let observed = observe();

    // SAFETY: as above. `sigwait` takes the delivery queued just now, which
    // is still blocked, so the call returns at once rather than waiting.
    unsafe {
        let mut taken: libc::c_int = 0;
        assert_eq!(libc::sigwait(&raw const blocked, &raw mut taken), 0);
        assert_eq!(taken, signal, "another signal was taken off the queue");
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const previous, std::ptr::null_mut());
    }

    observed
}
