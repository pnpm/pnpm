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

/// Let the process that `command` starts dump core, as far as the hard
/// limit it inherits allows.
///
/// For a test that asserts a core dump did *not* happen. Under a shell that
/// already set `ulimit -c 0` such an assertion holds whatever the code under
/// test does, so the test has to raise the limit for the child it inspects.
/// Both attributes survive the `exec`.
///
/// # Panics
///
/// When the hard limit forbids a dump outright, which leaves the assertion
/// vacuous and no test able to work around it.
#[cfg(unix)]
pub fn enable_core_dumps(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt as _;

    let mut inherited = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: the pointer is to a stack local that outlives the call, of
    // exactly the type `getrlimit` writes, and both of whose fields are
    // initialized beforehand so a call that fails without writing still
    // leaves the local readable.
    let read = unsafe { libc::getrlimit(libc::RLIMIT_CORE, &raw mut inherited) };
    assert_eq!(read, 0, "read RLIMIT_CORE: {}", std::io::Error::last_os_error());
    assert_ne!(
        inherited.rlim_max, 0,
        "a core-dump test needs a non-zero RLIMIT_CORE hard limit to inherit",
    );

    // SAFETY: the hook runs in the forked child, between the fork and the
    // `exec`, where only the calling thread exists and so only
    // async-signal-safe work is allowed. It calls `getrlimit`, `setrlimit`
    // and `prctl` and nothing else: syscall wrappers that allocate nothing,
    // take no lock, and write only a stack local that outlives each call.
    unsafe {
        command.pre_exec(|| {
            let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if libc::getrlimit(libc::RLIMIT_CORE, &raw mut limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            limit.rlim_cur = limit.rlim_max;
            if libc::setrlimit(libc::RLIMIT_CORE, &raw const limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            if libc::prctl(libc::PR_SET_DUMPABLE, 1) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
