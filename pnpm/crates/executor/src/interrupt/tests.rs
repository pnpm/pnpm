use super::interrupt_count;

/// The signal is aimed at this thread with the thread blocking it, so it
/// stays pending until the test takes it back.
#[test]
fn a_pending_signal_counts_as_an_interrupt() {
    // SAFETY: every call takes stack locals that outlive it, and the
    // signal goes to this thread only, which blocks it until `sigwait`
    // takes it back.
    unsafe {
        let mut hangup: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut hangup);
        libc::sigaddset(&raw mut hangup, libc::SIGHUP);
        let mut previous: libc::sigset_t = std::mem::zeroed();
        assert_eq!(libc::pthread_sigmask(libc::SIG_BLOCK, &raw const hangup, &raw mut previous), 0);

        let before = interrupt_count();
        assert_eq!(libc::pthread_kill(libc::pthread_self(), libc::SIGHUP), 0);
        let while_pending = interrupt_count();

        let mut taken: libc::c_int = 0;
        let waited = libc::sigwait(&raw const hangup, &raw mut taken);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const previous, std::ptr::null_mut());

        assert_eq!((waited, taken), (0, libc::SIGHUP));
        assert_eq!(while_pending, before + 1);
        assert_eq!(interrupt_count(), before);
    }
}
