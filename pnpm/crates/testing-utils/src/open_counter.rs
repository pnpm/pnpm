//! How many times a test's files are opened, counted by the kernel.
//!
//! A test that pins how often a command reads a file has no seam to count
//! through when the reads happen below the code under test, so it asks
//! `inotify` instead. Production never watches its own files this way, which
//! is what puts the calls here rather than in a `pnpm-ffi-*` crate.

use std::{ffi::CString, os::unix::ffi::OsStrExt as _, path::PathBuf};

/// How many times `paths` are opened while `run` runs.
///
/// Counts an open of any watched path, so a caller that wants them apart
/// watches one at a time. A path that cannot be watched panics rather than
/// going uncounted, which would read as "never opened".
pub fn count_opens(paths: &[PathBuf], run: impl FnOnce()) -> usize {
    // SAFETY: takes no pointer, and the descriptor it returns is checked
    // below before anything uses it.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    assert!(fd >= 0, "inotify_init1: {}", std::io::Error::last_os_error());
    for path in paths {
        let path = CString::new(path.as_os_str().as_bytes()).expect("a path without NUL");
        // SAFETY: `fd` is the descriptor just created, and `path` is a
        // NUL-terminated string that outlives the call.
        let watch = unsafe { libc::inotify_add_watch(fd, path.as_ptr(), libc::IN_OPEN) };
        assert!(watch >= 0, "inotify_add_watch: {}", std::io::Error::last_os_error());
    }
    run();
    let opens = read_open_events(fd);
    // SAFETY: `fd` is open here and closed exactly once, and nothing reads
    // it afterwards.
    unsafe { libc::close(fd) };
    opens
}

/// The `IN_OPEN` events queued on the inotify descriptor `fd`.
fn read_open_events(fd: libc::c_int) -> usize {
    const HEADER: usize = size_of::<libc::inotify_event>();
    let mut buffer = [0_u8; 4096];
    let mut opens = 0;
    loop {
        // SAFETY: `buffer` is writable for the whole length passed, and the
        // descriptor is non-blocking, so a read with nothing queued returns
        // an error rather than waiting.
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        let Ok(read) = usize::try_from(read) else { break };
        if read == 0 {
            break;
        }
        let mut offset = 0;
        while offset + HEADER <= read {
            // SAFETY: the kernel wrote a whole event header at `offset`,
            // which the loop condition keeps inside what the read returned,
            // and `read_unaligned` copes with the byte buffer's alignment.
            let event = unsafe {
                std::ptr::read_unaligned(buffer.as_ptr().add(offset).cast::<libc::inotify_event>())
            };
            if event.mask & libc::IN_OPEN != 0 {
                opens += 1;
            }
            offset += HEADER + event.len as usize;
        }
    }
    opens
}
