//! Whether a process a test recorded is still running.

/// Whether `pid` still runs.
///
/// A process that has exited but whose parent has not reaped it yet still
/// answers a probe, so where the kernel publishes a process state, as Linux
/// does under `/proc`, such a process reads as stopped. Elsewhere it reads
/// as running until it is reaped.
#[must_use]
pub fn is_running(pid: i32) -> bool {
    // SAFETY: `kill` with signal `0` only probes for the target's
    // existence, delivering nothing. It takes both arguments by value and
    // writes no memory.
    let alive = unsafe { libc::kill(pid, 0) == 0 };
    let reaped = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| {
            stat.rsplit_once(") ")
                .is_some_and(|(_, state)| state.starts_with('Z'))
        });
    alive && !reaped
}
