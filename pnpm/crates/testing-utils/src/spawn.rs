//! Spawn settings a test needs and the standard library does not offer.

/// Give a forked child the file-creation mask `umask` before it `exec`s.
///
/// For a test that has to install under a mask of its own choosing; the
/// mask is process-global, so setting it in the test's own process would
/// reach every other test in it.
#[cfg(unix)]
pub fn with_umask(command: &mut std::process::Command, umask: u32) {
    use std::os::unix::process::CommandExt as _;

    let umask = libc::mode_t::try_from(umask).expect("the mask fits mode_t");
    // SAFETY: the hook runs in the forked child, between the fork and the
    // `exec`, where only the calling thread exists and so only
    // async-signal-safe work is allowed. It calls `umask` and nothing else:
    // a syscall wrapper that allocates nothing, takes no lock, writes no
    // memory, and cannot fail or unwind.
    unsafe {
        command.pre_exec(move || {
            libc::umask(umask);
            Ok(())
        });
    }
}

/// Make the process that `command` starts adopt the orphans among its
/// descendants, as a container's PID 1 does.
///
/// For a test that needs that shape without becoming those orphans' parent
/// itself. The attribute survives the `exec`.
#[cfg(target_os = "linux")]
pub fn as_child_subreaper(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt as _;

    // SAFETY: the hook runs in the forked child, between the fork and the
    // `exec`, where only the calling thread exists and so only
    // async-signal-safe work is allowed. It calls `prctl` and nothing else:
    // a syscall wrapper that allocates nothing, takes no lock, and writes
    // no memory for this request.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
