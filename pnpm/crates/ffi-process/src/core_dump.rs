//! Whether the calling process dumps core: `RLIMIT_CORE`, and on Linux the
//! dumpable flag a `core_pattern` helper consults.

/// Stop the process from dumping core.
///
/// Best-effort: a limit that cannot be lowered is left alone, since a
/// caller suppressing a dump is on its way to dying and has no second
/// course to take.
///
/// `RLIMIT_CORE` stops file-based dumps across Unix. Linux's `do_coredump`
/// ignores it when `core_pattern` pipes to a helper such as
/// `systemd-coredump`, so the dumpable flag is cleared there as well.
///
/// Async-signal-safe.
pub fn suppress_core_dump() {
    let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: the pointer is to a fully initialized stack local that
    // outlives the call, of exactly the type `setrlimit` reads. The call
    // retains no pointer.
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &raw const none) };
    // SAFETY: `PR_SET_DUMPABLE` reads its one argument, writes no memory,
    // and takes no pointer.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0)
    };
}
