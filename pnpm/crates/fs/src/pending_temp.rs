//! Temp files of in-flight atomic writes, unlinked when the process is
//! interrupted.
//!
//! An atomic write stages its content in a temp file next to the target and
//! publishes it by `rename`. A signal — `Ctrl+C` at the terminal, a service
//! manager's `SIGTERM` — takes the process down without running destructors,
//! so a temp file caught mid-write would stay behind in the project
//! ([pnpm/pnpm#1418](https://github.com/pnpm/pnpm/issues/1418)). Writers
//! register each staged temp file with [`track_temp_file`]; the handler this
//! module installs unlinks whatever is still pending before the signal ends
//! the process.
//!
//! The handler chains to the disposition it replaced rather than assuming
//! the default: `pnpm-executor`'s interrupt relay may have been installed
//! first and may keep the process alive while children settle, so the
//! handler unlinks only when the process is about to die — before the
//! reset-and-raise for the default disposition, or in the relay's own exit
//! path, which cleans this registry in turn. Unlinking while the relay
//! keeps waiting would pull temp files out from under writes that keep
//! running. An interrupt thus both reaches the children pnpm started and
//! removes the temp files, whichever order the two handlers were installed
//! in.

use std::{
    path::Path,
    ptr,
    sync::{
        Once,
        atomic::{AtomicPtr, AtomicUsize, Ordering},
    },
};

/// The terminal and service-manager signals that end pnpm without running
/// destructors.
#[cfg(unix)]
const SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

/// The disposition each handled signal had before this module's handler was
/// installed, so the handler can pass the signal on after cleaning up.
/// `SIG_DFL` (0) means death by the signal; anything else is a handler to
/// call, as `SIG_IGN` dispositions are left untouched at install time.
#[cfg(unix)]
static PREVIOUS: [AtomicUsize; SIGNALS.len()] = [const { AtomicUsize::new(0) }; SIGNALS.len()];

/// The capacity of an entry's path buffer, NUL included. Covers `PATH_MAX`
/// on Linux and macOS; a longer path stays untracked rather than being
/// truncated into the name of a different file.
const MAX_STORED_PATH: usize = 4096;

/// A claim in progress: the slot's buffer is being overwritten, so the
/// cleanup must skip it. No real path length collides with the sentinel.
const CLAIMING: usize = usize::MAX;

/// One staged temp file: the path bytes to unlink on interrupt, NUL
/// terminated, with `len` 0 while the slot is free and [`CLAIMING`] while a
/// claim writes them.
///
/// Entries form a list the signal handler walks, so they are leaked rather
/// than freed, and a released slot — buffer included — is reused by the
/// next write, keeping the footprint bounded by the peak number of
/// concurrent writes rather than the total. The handler then only reads
/// atomics and unlinks, which is async-signal-safe, while the list itself
/// has no fixed size.
struct Entry {
    len: AtomicUsize,
    buf: [u8; MAX_STORED_PATH],
    next: AtomicPtr<Entry>,
}

static HEAD: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());

/// A staged temp file's registration. Dropping it releases the slot: the
/// write has published or removed the temp file itself.
pub struct PendingTempFile {
    entry: Option<&'static Entry>,
}

impl Drop for PendingTempFile {
    fn drop(&mut self) {
        if let Some(entry) = self.entry {
            entry.len.store(0, Ordering::Release);
        }
    }
}

/// Register `path` as a temp file to unlink if the process is interrupted,
/// until the returned guard is dropped.
#[must_use]
pub fn track_temp_file(path: &Path) -> PendingTempFile {
    install_handler();
    let Some(bytes) = path_bytes(path) else {
        return PendingTempFile { entry: None };
    };
    if bytes.len() >= MAX_STORED_PATH {
        return PendingTempFile { entry: None };
    }
    PendingTempFile { entry: Some(claim_entry(bytes)) }
}

/// Unlink every temp file still registered.
///
/// Async-signal-safe: the walk only reads atomics and unlinks. Called from
/// this module's own handler on the path that ends the process, and by
/// `pnpm-executor`'s relay when it is the handler that ends the process.
pub fn remove_pending_temp_files() {
    let mut next = HEAD.load(Ordering::Acquire);
    // SAFETY: every pointer in the list came from the `Box::leak` in
    // `push_entry` and is never freed, so it stays dereferenceable.
    while let Some(entry) = unsafe { next.as_ref() } {
        let len = entry.len.load(Ordering::Acquire);
        if len != 0 && len != CLAIMING {
            unlink_entry(entry, len);
        }
        next = entry.next.load(Ordering::Acquire);
    }
}

/// Take the first free slot for `bytes`, or extend the list with one. The
/// length is published only after the buffer holds the path, so a handler
/// that reads a nonzero length after acquiring it sees the whole path.
fn claim_entry(bytes: &[u8]) -> &'static Entry {
    let mut next = HEAD.load(Ordering::Acquire);
    // SAFETY: as in `remove_pending_temp_files`, list entries are leaked and
    // stay dereferenceable for the life of the process.
    while let Some(entry) = unsafe { next.as_ref() } {
        if entry.len.compare_exchange(0, CLAIMING, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // SAFETY: the claim makes the slot this thread's alone until
            // `len` is published. A handler that read the previous length
            // before the slot was released may still be reading; it then
            // unlinks a mix of two of this process's own temp paths, which
            // at worst removes a temp file of a concurrent write.
            unsafe {
                let buf = ptr::from_ref(&entry.buf).cast_mut().cast::<u8>();
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(bytes.len()) = 0;
            }
            entry.len.store(bytes.len(), Ordering::Release);
            return entry;
        }
        next = entry.next.load(Ordering::Acquire);
    }
    push_entry(bytes)
}

/// Add an entry for `bytes` at the head of the list. The buffer and length
/// are set before the head swings, so a handler walking the list
/// concurrently either misses the new entry entirely or reads a complete
/// one.
fn push_entry(bytes: &[u8]) -> &'static Entry {
    let mut buf = [0; MAX_STORED_PATH];
    buf[..bytes.len()].copy_from_slice(bytes);
    let entry: &'static Entry = Box::leak(Box::new(Entry {
        len: AtomicUsize::new(bytes.len()),
        buf,
        next: AtomicPtr::new(ptr::null_mut()),
    }));
    let mut head = HEAD.load(Ordering::Acquire);
    loop {
        entry.next.store(head, Ordering::Release);
        match HEAD.compare_exchange_weak(
            head,
            ptr::from_ref(entry).cast_mut(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return entry,
            Err(current) => head = current,
        }
    }
}

/// The path as the cleanup consumes it: raw bytes on Unix, where the reader
/// is a signal handler passing them straight to `unlink`.
#[cfg(unix)]
fn path_bytes(path: &Path) -> Option<&[u8]> {
    use std::os::unix::ffi::OsStrExt as _;

    Some(path.as_os_str().as_bytes())
}

/// The console-handler thread reconstructs the path from UTF-8; a
/// non-UTF-8 path stays untracked rather than being lossily rewritten.
#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Option<&[u8]> {
    path.to_str().map(str::as_bytes)
}

#[cfg(unix)]
fn unlink_entry(entry: &Entry, _len: usize) {
    // SAFETY: a published slot's buffer holds the path bytes followed by a
    // NUL, and `unlink` is async-signal-safe.
    unsafe {
        libc::unlink(entry.buf.as_ptr().cast());
    }
}

#[cfg(not(unix))]
fn unlink_entry(entry: &Entry, len: usize) {
    // SAFETY: the buffer stays dereferenceable for the life of the process,
    // and a published length names bytes written before the publication.
    let bytes = unsafe { std::slice::from_raw_parts(entry.buf.as_ptr(), len) };
    if let Ok(path) = std::str::from_utf8(bytes) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
fn install_handler() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        for (index, signal) in SIGNALS.iter().enumerate() {
            install_handler_for(index, *signal);
        }
    });
}

#[cfg(unix)]
fn install_handler_for(index: usize, signal: libc::c_int) {
    // SAFETY: both calls take stack locals that outlive them, and a null
    // `act` only reads the current disposition back.
    unsafe {
        let mut previous: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(signal, ptr::null(), &raw mut previous) != 0 {
            return;
        }
        // A signal pnpm was started with ignored stays ignored, so pnpm
        // under `nohup` or in a background job keeps that disposition.
        if previous.sa_sigaction == libc::SIG_IGN {
            return;
        }
        PREVIOUS[index].store(previous.sa_sigaction, Ordering::Release);
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = clean_temp_files as *const () as usize;
        // `SA_RESTART` keeps the in-flight syscalls underneath from failing
        // with `EINTR` when the handler fires.
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&raw mut action.sa_mask);
        libc::sigaction(signal, &raw const action, ptr::null_mut());
    }
}

/// Pass the signal on to the disposition this handler replaced, unlinking
/// the pending temp files only on the path that ends the process: the
/// default dies by the signal, so it gets the cleanup first, while a
/// handler installed earlier — `pnpm-executor`'s interrupt relay — may keep
/// pnpm alive waiting for children, and unlinking now would pull temp files
/// out from under writes that keep running. The relay's own exit path calls
/// [`remove_pending_temp_files`] when it ends the process.
///
/// Everything up to the chained call is async-signal-safe: atomic loads and
/// `unlink`.
#[cfg(unix)]
extern "C" fn clean_temp_files(signal: libc::c_int) {
    let Some(index) = SIGNALS
        .iter()
        .position(|candidate| *candidate == signal)
    else {
        remove_pending_temp_files();
        return;
    };
    let previous = PREVIOUS[index].load(Ordering::Acquire);
    if previous == libc::SIG_DFL {
        remove_pending_temp_files();
        die_from(signal);
    }
    if previous == libc::SIG_IGN {
        // Never installed over an ignored signal, so unreachable; treating
        // it as handled beats calling address 1 if that ever changes.
        return;
    }
    // SAFETY: `previous` is the `sa_sigaction` read back at install time of
    // a disposition that was neither the default nor ignore, so it holds a
    // handler installed without `SA_SIGINFO` — an `extern "C" fn(c_int)`
    // that outlives the process. Nothing in this process installs a
    // three-argument `SA_SIGINFO` handler for these signals.
    let handler: extern "C" fn(libc::c_int) = unsafe { std::mem::transmute(previous) };
    handler(signal);
}

/// End the process as `signal` would have ended it without this module.
///
/// The signal is unblocked first because a handler runs with its own
/// signal blocked: `raise` would otherwise leave it pending until the
/// handler returned, and the `_exit` below would report a plain exit code
/// where the caller expects death by a signal.
#[cfg(unix)]
fn die_from(signal: libc::c_int) -> ! {
    // SAFETY: `sigprocmask`, `signal`, `raise` and `_exit` are all
    // async-signal-safe, and the set is a stack local that outlives the
    // call. `raise` does not return once the signal is unblocked and back
    // at its default disposition; `_exit` covers the impossible case.
    unsafe {
        let mut unblocked: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut unblocked);
        libc::sigaddset(&raw mut unblocked, signal);
        libc::signal(signal, libc::SIG_DFL);
        libc::sigprocmask(libc::SIG_UNBLOCK, &raw const unblocked, ptr::null_mut());
        libc::raise(signal);
        libc::_exit(128 + signal);
    }
}

#[cfg(windows)]
fn install_handler() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        // SAFETY: the handler is a plain `extern "system"` function whose
        // only state is this module's atomics.
        unsafe {
            SetConsoleCtrlHandler(Some(clean_temp_files), 1);
        }
    });
}

/// Remove the pending temp files, then pass the event on: returning `FALSE`
/// lets the next handler — `pnpm-executor`'s relay while children are
/// running, or the default termination — decide how the process ends.
///
/// Unlike the Unix handler this cannot defer the cleanup to the exit path:
/// a console handler runs on a thread of its own and never learns whether
/// the next handler kept the process alive, and the relay's Windows exit
/// path is a plain exit code with no cleanup point of its own.
#[cfg(windows)]
unsafe extern "system" fn clean_temp_files(event: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    if !matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        return 0;
    }
    remove_pending_temp_files();
    0
}

#[cfg(not(any(unix, windows)))]
fn install_handler() {}

#[cfg(test)]
mod tests;
