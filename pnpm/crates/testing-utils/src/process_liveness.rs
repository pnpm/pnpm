//! Whether a process a test recorded is still running, and waiting
//! until it is not.

/// Whether `pid` still runs.
///
/// A process that has exited but whose parent has not reaped it yet still
/// answers a probe, so where the kernel publishes a process state, as Linux
/// does under `/proc`, such a process reads as stopped. Elsewhere it reads
/// as running until it is reaped.
#[must_use]
pub fn is_running(pid: i32) -> bool {
    // Read the state before probing. A process reaped between the probe and
    // the read answers the probe and then has no state left to read, which
    // would make a process that is gone read as running.
    let reaped = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| {
            stat.rsplit_once(") ")
                .is_some_and(|(_, state)| state.starts_with('Z'))
        });
    // SAFETY: `kill` with signal `0` only probes for the target's
    // existence, delivering nothing. It takes both arguments by value and
    // writes no memory.
    let alive = unsafe { libc::kill(pid, 0) == 0 };
    alive && !reaped
}

/// Wait for `pid` to exit, leaving its exit status for its owner to reap.
///
/// For a test that kills a child the code under test still owns. The kill
/// returns before the process is gone, and [`is_running`] cannot close
/// that window portably: a child that has exited unreaped still answers a
/// probe everywhere but Linux. `WNOWAIT` observes the exit without
/// consuming the status, so the owner's own wait still finds it.
pub fn wait_for_exit_without_reaping(pid: i32) {
    let pid = libc::id_t::try_from(pid).expect("a process id is not negative");
    // SAFETY: `waitid` writes only the `siginfo_t` behind the pointer, a
    // stack local that outlives the call and is zeroed beforehand, so a
    // call that returns without writing still leaves it readable.
    // `WNOWAIT` leaves the child's status in place, so this reaps nothing
    // that another owner is waiting for.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        libc::waitid(libc::P_PID, pid, &raw mut info, libc::WEXITED | libc::WNOWAIT);
    }
}
