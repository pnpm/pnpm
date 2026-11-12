use super::{Command, GroupWatchdog};
use std::{
    io::{self, Read},
    os::fd::{AsRawFd, RawFd},
    process::Child,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) fn registered(command: &mut Command, watchdog: &GroupWatchdog) -> io::Result<Child> {
    let (mut reader, writer) = {
        // macOS sets CLOEXEC after creating a pipe; exclude concurrent forks.
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let _spawning = pnpm_process::watchdog_spawning();
        io::pipe()?
    };
    let active = Arc::new(AtomicBool::new(true));
    let hook_active = Arc::clone(&active);
    let lifeline = watchdog.lifeline.as_raw_fd();
    let receipt = writer.as_raw_fd();
    // SAFETY: the hook uses only atomic loads, stack arithmetic and async-signal-safe
    // getpid/write calls. Both descriptors stay open until spawn returns. The hook
    // is disabled before they close, including when the Command is reused.
    unsafe {
        command.pre_exec(move || {
            if hook_active.load(Ordering::SeqCst) {
                register(lifeline, receipt)?;
            }
            Ok(())
        });
    }
    let spawned = command.spawn();
    active.store(false, Ordering::SeqCst);
    drop(writer);
    if spawned.is_err() {
        let mut bytes = [0; size_of::<u32>()];
        match reader.read_exact(&mut bytes) {
            Ok(()) => watchdog.release(u32::from_ne_bytes(bytes))?,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(error) => return Err(error),
        }
    }
    spawned
}

fn register(lifeline: RawFd, receipt: RawFd) -> io::Result<()> {
    // SAFETY: getpid has no preconditions and is async-signal-safe.
    let leader = unsafe { libc::getpid() } as u32;
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
        // SAFETY: the descriptor is open and bytes stays valid for the write.
        // The message fits PIPE_BUF, so a successful pipe write is atomic.
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

#[cfg(test)]
mod tests;
