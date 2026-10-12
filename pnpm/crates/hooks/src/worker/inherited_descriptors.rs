//! The worker lives until pnpm exits, so it must not hold a descriptor
//! that pnpm waits on.
//!
//! macOS has no `pipe2`. The standard library opens a child's pipes and
//! marks them close-on-exec in a second step, so a worker spawned by
//! another thread between the two steps inherits them. When the pipe is the
//! stdout of a probe that pnpm reads to EOF, such as `node --version`, pnpm
//! waits for the worker and the worker waits for pnpm.

use crate::async_process::Command;
use std::{io, mem::size_of};

/// Room for the descriptors pnpm opens between sizing the list and forking.
const SPARE_DESCRIPTORS: usize = 256;

/// Make every descriptor the worker inherits close on exec, except its
/// stdio.
pub(super) fn inherit_only_stdio(command: &mut Command) {
    let mut descriptors =
        vec![libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 }; descriptor_capacity()];
    // SAFETY: the hook runs in the forked child before exec. It calls only
    // `getpid`, `proc_pidinfo`, `getdtablesize` and `fcntl`, which neither
    // allocate nor lock, and writes only into the list allocated before the
    // fork.
    unsafe {
        command.pre_exec(move || {
            mark_close_on_exec_above_stdio(&mut descriptors);
            Ok::<(), io::Error>(())
        });
    }
}

fn descriptor_capacity() -> usize {
    // SAFETY: a null buffer asks only for the size of the current list.
    let bytes = unsafe {
        libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0)
    };
    usize::try_from(bytes).unwrap_or(0) / size_of::<libc::proc_fdinfo>() * 2 + SPARE_DESCRIPTORS
}

/// Runs between fork and exec, so it must stay async-signal-safe.
fn mark_close_on_exec_above_stdio(descriptors: &mut [libc::proc_fdinfo]) {
    let capacity = libc::c_int::try_from(size_of_val(descriptors)).unwrap_or(libc::c_int::MAX);
    // SAFETY: `descriptors` is writable for `capacity` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            capacity,
        )
    };
    let listed = usize::try_from(written).unwrap_or(0) / size_of::<libc::proc_fdinfo>();
    let mut highest_listed = libc::STDERR_FILENO;
    for descriptor in &descriptors[..listed] {
        mark_close_on_exec(descriptor.proc_fd);
        highest_listed = highest_listed.max(descriptor.proc_fd);
    }
    // A full list may be cut short, and an empty one means the call failed:
    // check every descriptor number past the listed ones.
    if listed == 0 || listed == descriptors.len() {
        // SAFETY: no arguments.
        let table_size = unsafe { libc::getdtablesize() };
        for descriptor in highest_listed + 1..table_size {
            mark_close_on_exec(descriptor);
        }
    }
}

fn mark_close_on_exec(descriptor: libc::c_int) {
    if descriptor <= libc::STDERR_FILENO {
        return;
    }
    // SAFETY: `fcntl` on a closed descriptor fails with `EBADF`.
    unsafe {
        let flags = libc::fcntl(descriptor, libc::F_GETFD);
        if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
            libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}
