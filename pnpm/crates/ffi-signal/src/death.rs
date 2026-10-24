//! Ending the process the way a signal would have ended it.

/// End the process as `signal` would have, with no handler installed.
///
/// For a handler that has decided the process dies, and for reporting a
/// child's death by a signal as pnpm's own. The process reports death by
/// the signal rather than an exit code, which is what a shell needs to
/// print `^C` and skip the rest of a `&&` chain.
///
/// Async-signal-safe.
pub fn die_from(signal: i32) -> ! {
    // A handler runs with its own signal blocked, so `raise` would leave it
    // pending until the handler returned and the `_exit` below would report
    // a plain exit code where the caller promised death by a signal.
    // Unblocking it first is what makes the `raise` take effect here.
    //
    // SAFETY: `sigprocmask`, `signal`, `raise` and `_exit` are all
    // async-signal-safe, and the set is a fully initialized stack local
    // that outlives the calls that read it. `raise` does not return once
    // the signal is unblocked and back at its default disposition; `_exit`
    // covers the case where it somehow does.
    unsafe {
        let mut unblocked: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut unblocked);
        libc::sigaddset(&raw mut unblocked, signal);
        libc::signal(signal, libc::SIG_DFL);
        libc::sigprocmask(libc::SIG_UNBLOCK, &raw const unblocked, std::ptr::null_mut());
        libc::raise(signal);
        libc::_exit(128 + signal);
    }
}
