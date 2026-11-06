//! Keeping a forked child from inheriting descriptors that are not its
//! stdio.
//!
//! macOS has no `pipe2`. The standard library opens a child's pipes and
//! marks them close-on-exec in a second step, so a process spawned by
//! another thread between the two steps inherits them. When the pipe is the
//! stdout of a probe that pnpm reads to EOF, such as `node --version`, pnpm
//! waits for the long-lived child and the child waits for pnpm.

use std::{io, mem::size_of, process::Command};

/// Room for the descriptors the process opens between sizing the list and
/// forking. The list is allocated before the fork, because the child
/// cannot allocate, so it has to be big enough for a little growth.
const SPARE_DESCRIPTORS: usize = 256;

/// Arrange for `command`'s child to close every descriptor it inherits
/// except its stdio, between the fork and the `exec`.
pub fn inherit_only_stdio(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    let mut descriptors =
        vec![libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 }; descriptor_capacity()];
    // SAFETY: the hook runs in the forked child, between the fork and the
    // `exec`, where only the calling thread exists and so only
    // async-signal-safe work is allowed. It allocates nothing: the list it
    // fills was allocated above, before the fork. It takes no lock, and it
    // calls only `getpid`, `proc_pidinfo`, `getdtablesize` and `fcntl`,
    // each of which is a syscall wrapper that neither allocates nor locks.
    // It cannot unwind: the slice it indexes is bounded by the length the
    // call reports, which `listed` clamps to the list's own length.
    unsafe {
        command.pre_exec(move || {
            close_above_stdio(&mut descriptors);
            Ok::<(), io::Error>(())
        });
    }
}

/// How long the descriptor list has to be: twice what the process has open
/// now, plus room for a burst.
fn descriptor_capacity() -> usize {
    // SAFETY: a null buffer with a zero length asks only for the size the
    // list currently needs, which is the documented way to size the buffer
    // below. `getpid` takes no arguments and cannot fail.
    let bytes = unsafe {
        libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0)
    };
    usize::try_from(bytes).unwrap_or(0) / size_of::<libc::proc_fdinfo>() * 2 + SPARE_DESCRIPTORS
}

/// Runs between the fork and the `exec`, so everything it calls has to be
/// async-signal-safe.
fn close_above_stdio(descriptors: &mut [libc::proc_fdinfo]) {
    let capacity = libc::c_int::try_from(size_of_val(descriptors)).unwrap_or(libc::c_int::MAX);
    // SAFETY: the pointer is to a live slice of `capacity` writable bytes,
    // which is the capacity the call is told it has, and `getpid` cannot
    // fail.
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            capacity,
        )
    };
    let listed = (usize::try_from(written).unwrap_or(0) / size_of::<libc::proc_fdinfo>()).min(
        descriptors.len(),
    );
    let mut highest_listed = libc::STDERR_FILENO;
    for descriptor in &descriptors[..listed] {
        close_on_exec(descriptor.proc_fd);
        highest_listed = highest_listed.max(descriptor.proc_fd);
    }
    // A full list may have been cut short, and an empty one means the call
    // failed, so in both cases every descriptor number past the listed ones
    // is checked.
    if listed == 0 || listed == descriptors.len() {
        // SAFETY: `getdtablesize` takes no arguments, writes no memory, and
        // cannot fail.
        let table_size = unsafe { libc::getdtablesize() };
        for descriptor in highest_listed + 1..table_size {
            close_on_exec(descriptor);
        }
    }
}

fn close_on_exec(descriptor: libc::c_int) {
    if descriptor <= libc::STDERR_FILENO {
        return;
    }
    // SAFETY: `fcntl` takes its arguments by value and writes no memory for
    // either of these commands. A descriptor that is not open is reported
    // with `EBADF` rather than acted on, which the `>= 0` check below
    // handles.
    unsafe {
        let flags = libc::fcntl(descriptor, libc::F_GETFD);
        if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
            libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}
