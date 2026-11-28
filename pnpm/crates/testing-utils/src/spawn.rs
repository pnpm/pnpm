//! Spawn settings a test needs and the standard library does not offer.

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

/// Hold a forked child between `fork` and `exec` until it is let go.
///
/// The child writes one byte to `ready`, then blocks reading `gate`, so a
/// test can see a spawn in flight and decide when it finishes. For a test
/// about what happens while a spawn is in progress, which is a state a
/// caller cannot otherwise hold still.
#[cfg(unix)]
pub fn pause_before_exec(
    command: &mut std::process::Command,
    ready: std::os::fd::RawFd,
    gate: std::os::fd::RawFd,
) {
    use std::os::unix::process::CommandExt as _;

    // SAFETY: the hook runs in the forked child, between the fork and the
    // `exec`, where only the calling thread exists and so only
    // async-signal-safe work is allowed. It calls `write` and `read` and
    // nothing else, over stack buffers that outlive both, and allocates
    // nothing and takes no lock. The caller keeps both descriptors open
    // across the spawn.
    unsafe {
        command.pre_exec(move || wait_for_gate(ready, gate));
    }
}

/// Report being between `fork` and `exec`, then wait to be let go.
#[cfg(unix)]
fn wait_for_gate(ready: std::os::fd::RawFd, gate: std::os::fd::RawFd) -> std::io::Result<()> {
    let mut byte = 1u8;
    // SAFETY: both descriptors are open for the calls, and the buffer is a
    // stack local that outlives them. Each call moves one byte.
    let succeeded = unsafe {
        libc::write(ready, (&raw const byte).cast(), 1) == 1
            && libc::read(gate, (&raw mut byte).cast(), 1) == 1
    };
    if succeeded { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Run `command` to completion, retrying while `exec` fails with `ETXTBSY`.
///
/// For a test that runs an executable it has just written. A child that a
/// concurrent test forks in the meantime inherits the write descriptor and
/// holds it until its own `exec`, and Linux refuses to run a file open for
/// writing.
///
/// # Panics
///
/// When the spawn fails otherwise, or the file is still busy after two
/// seconds.
pub fn output_retrying_busy(command: &mut std::process::Command) -> std::process::Output {
    let mut last_busy = None;
    for _ in 0..200 {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test helper starts processes the executor does not run"
        )]
        match command.output() {
            Ok(output) => return output,
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                last_busy = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("run {command:?}: {error:?}"),
        }
    }
    panic!("{command:?} stayed busy: {last_busy:?}");
}
