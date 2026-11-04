//! The paths a command opens, observed through inotify. No filesystem seam
//! reaches every read the CLI makes, so the kernel observes them instead.

use std::{
    collections::HashMap,
    ffi::{CString, OsStr},
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
};

/// Every open of a path in `watched` while `run` runs, in order, one entry
/// per open. A watched directory reports the opens of its entries, and of
/// itself when it is listed. Its subdirectories are not watched.
pub fn record_opens(watched: &[PathBuf], run: impl FnOnce()) -> Vec<PathBuf> {
    // SAFETY: no pointer arguments; a negative result is checked below.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    assert!(fd >= 0, "inotify_init1: {}", std::io::Error::last_os_error());
    let watches: HashMap<libc::c_int, &Path> = watched
        .iter()
        .map(|path| (add_watch(fd, path), path.as_path()))
        .collect();
    run();
    let opens = read_open_events(fd, &watches);
    // SAFETY: `fd` is open and closed once.
    unsafe { libc::close(fd) };
    opens
}

fn add_watch(fd: libc::c_int, path: &Path) -> libc::c_int {
    let path = CString::new(path.as_os_str().as_bytes()).expect("a path without NUL");
    // The kernel merges an event into an identical one left unread at the
    // tail of the queue, so the closes keep back-to-back opens apart.
    let mask = libc::IN_OPEN | libc::IN_CLOSE;
    // SAFETY: `fd` is an inotify descriptor and `path` outlives the call.
    let watch = unsafe { libc::inotify_add_watch(fd, path.as_ptr(), mask) };
    assert!(watch >= 0, "inotify_add_watch: {}", std::io::Error::last_os_error());
    watch
}

/// The paths of the `IN_OPEN` events queued on the inotify descriptor `fd`.
fn read_open_events(fd: libc::c_int, watches: &HashMap<libc::c_int, &Path>) -> Vec<PathBuf> {
    const HEADER: usize = size_of::<libc::inotify_event>();
    let mut buffer = [0_u8; 4096];
    let mut opens = Vec::new();
    loop {
        // SAFETY: `buffer` is writable for its whole length.
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        let Ok(read) = usize::try_from(read) else { break };
        if read == 0 {
            break;
        }
        let mut offset = 0;
        while offset + HEADER <= read {
            // SAFETY: the kernel wrote a whole event header at `offset`, and
            // `read_unaligned` copes with the byte buffer's alignment.
            let event = unsafe {
                std::ptr::read_unaligned(
                    buffer
                        .as_ptr()
                        .add(offset)
                        .cast::<libc::inotify_event>(),
                )
            };
            assert_eq!(event.mask & libc::IN_Q_OVERFLOW, 0, "the inotify queue overflowed");
            let name = &buffer[offset + HEADER..offset + HEADER + event.len as usize];
            if event.mask & libc::IN_OPEN != 0 {
                opens.push(event_path(watches[&event.wd], name));
            }
            offset += HEADER + event.len as usize;
        }
    }
    opens
}

/// The path an event names: the watched path itself, or the entry of the
/// watched directory named by the NUL-padded `name`.
fn event_path(watched: &Path, name: &[u8]) -> PathBuf {
    let name = name
        .split(|byte| *byte == 0)
        .next()
        .unwrap_or_default();
    if name.is_empty() { watched.to_path_buf() } else { watched.join(OsStr::from_bytes(name)) }
}
