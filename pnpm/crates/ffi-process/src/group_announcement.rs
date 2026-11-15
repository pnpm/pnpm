//! Letting a child announce the process group it leads before it `exec`s.
//!
//! A child's process group has to reach the watchdog before the child can
//! start anything of its own, and only the child knows its own id that
//! early: the parent learns it when `spawn` returns, by which time the
//! child may already have started children of its own. So the child says
//! so itself, in the window between `fork` and `exec`.

use std::{
    io::{self, Read as _},
    mem::size_of,
    os::fd::{AsRawFd as _, BorrowedFd, RawFd},
    process::{Child, Command},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Spawn `command` through `spawn`, with its child announcing the process
/// group it leads before it `exec`s.
///
/// The child writes that group twice: to `lifeline` as `+<group>\n`, which
/// is how the watchdog learns of a group it must end if pnpm dies, and to
/// a pipe this call owns, which is how a spawn that fails afterwards is
/// undone.
///
/// `spawn` performs the spawn itself, so that a caller holding a lock
/// across it keeps that lock. It runs in this process, not in the child,
/// and so carries none of the child's restrictions.
///
/// Returns what `spawn` gave, and the announced group when the spawn
/// failed after the child had announced one. That group will never exist,
/// so a caller holding the watchdog withdraws it.
pub fn spawn_announcing_group(
    command: &mut Command,
    lifeline: BorrowedFd<'_>,
    spawn: impl FnOnce(&mut Command) -> io::Result<Child>,
) -> (io::Result<Child>, Option<u32>) {
    use std::os::unix::process::CommandExt as _;

    let (mut reader, writer) = match io::pipe() {
        Ok(pipe) => pipe,
        Err(error) => return (Err(error), None),
    };
    let armed = Arc::new(AtomicBool::new(true));
    let hook_armed = Arc::clone(&armed);
    let lifeline = lifeline.as_raw_fd();
    let receipt = writer.as_raw_fd();
    // SAFETY: the hook runs in the forked child, between the fork and the
    // `exec`, where only the calling thread exists and so only
    // async-signal-safe work is allowed. It allocates nothing and takes no
    // lock: it loads an atomic, reads `getpid`, formats into a stack array,
    // and calls `write`, each of which is a syscall wrapper or arithmetic.
    // It cannot unwind, every index into the array being bounded by its own
    // length. Both descriptors stay open until `spawn` below returns,
    // because this call owns one and borrows the other for its whole body,
    // and the hook is disarmed before either closes.
    unsafe {
        command.pre_exec(move || {
            if hook_armed.load(Ordering::SeqCst) {
                announce(lifeline, receipt)?;
            }
            Ok(())
        });
    }
    let spawned = spawn(command);
    // Disarm before the write end closes: its number is free to be handed
    // to the next thing this process opens, and a hook left armed on a
    // reused `Command` would write a group id into that instead.
    armed.store(false, Ordering::SeqCst);
    drop(writer);
    if spawned.is_err() {
        let mut bytes = [0; size_of::<u32>()];
        match reader.read_exact(&mut bytes) {
            Ok(()) => return (spawned, Some(u32::from_ne_bytes(bytes))),
            // Nothing was written and every writer is closed, so the child
            // never got as far as announcing itself.
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(error) => return (Err(error), None),
        }
    }
    (spawned, None)
}

/// Write the calling process's own group to both descriptors.
fn announce(lifeline: RawFd, receipt: RawFd) -> io::Result<()> {
    // `std::process::id` is a bare `getpid`, which is what keeps this
    // async-signal-safe: it allocates nothing and takes no lock.
    let leader = std::process::id();
    write(receipt, &leader.to_ne_bytes())?;
    let mut line = [0; 12];
    line[11] = b'\n';
    let mut offset = 11;
    let mut digits = leader;
    loop {
        offset -= 1;
        line[offset] = b'0' + (digits % 10) as u8;
        digits /= 10;
        if digits == 0 {
            break;
        }
    }
    offset -= 1;
    line[offset] = b'+';
    write(lifeline, &line[offset..])
}

fn write(descriptor: RawFd, bytes: &[u8]) -> io::Result<()> {
    loop {
        // SAFETY: the descriptor is open for the call, and `bytes` is
        // borrowed for it. The message fits `PIPE_BUF`, so a successful
        // pipe write is atomic and leaves no partial line for a reader.
        let written = unsafe { libc::write(descriptor, bytes.as_ptr().cast(), bytes.len()) };
        if written == bytes.len() as isize {
            return Ok(());
        }
        if written >= 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
