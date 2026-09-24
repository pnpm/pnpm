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
//! `pnpm-executor`'s interrupt relay handles the same signals and may keep
//! the process alive while children settle. It calls
//! [`install_temp_file_cleanup`] before installing itself, so the two
//! installs never race and the relay is the handler a signal reaches first.
//! The relay ends the process through [`die_from_signal`], which unlinks
//! the pending temp files; while it keeps waiting, the files stay, as
//! unlinking them would pull them out from under writes that keep running.
//! This handler chains to whatever disposition it replaced for the same
//! reason, unlinking only before the reset-and-raise of the default one.

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

/// The registered path as the cleanup reads it: a C string on Unix, where
/// the reader is a signal handler and only `unlink` is async-signal-safe.
#[cfg(unix)]
type StoredPath = std::ffi::CString;

#[cfg(not(unix))]
type StoredPath = std::path::PathBuf;

/// How many cleanup walks are running. A released path is freed only while
/// this is zero; otherwise a walk may still hold its pointer, and the path
/// is leaked instead. All accesses are `SeqCst`, so a walk that starts after
/// a release has seen zero here reads the released slot as empty.
static ACTIVE_WALKS: AtomicUsize = AtomicUsize::new(0);

/// One staged temp file: the path to unlink on interrupt, or null while the
/// slot is free.
///
/// Entries form a list the signal handler walks, so they are leaked rather
/// than freed, and a released slot is reused by the next write, so the list
/// grows only to the peak number of concurrent writes. The handler then only
/// touches atomics and unlinks, which is async-signal-safe.
struct Entry {
    path: AtomicPtr<StoredPath>,
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
        let Some(entry) = self.entry else {
            return;
        };
        let released = entry.path.swap(ptr::null_mut(), Ordering::SeqCst);
        if ACTIVE_WALKS.load(Ordering::SeqCst) == 0 {
            // SAFETY: `released` came from the `Box::into_raw` in
            // `track_temp_file`, only this guard releases it, and no walk
            // holds it: see `ACTIVE_WALKS`.
            drop(unsafe { Box::from_raw(released) });
        }
    }
}

/// Register `path` as a temp file to unlink if the process is interrupted,
/// until the returned guard is dropped.
#[must_use]
pub fn track_temp_file(path: &Path) -> PendingTempFile {
    install_temp_file_cleanup();
    let Some(stored) = store_path(path) else {
        return PendingTempFile { entry: None };
    };
    PendingTempFile { entry: Some(claim_entry(Box::into_raw(Box::new(stored)))) }
}

/// Unlink every temp file still registered.
///
/// Async-signal-safe: the walk only touches atomics and unlinks. Called
/// from this module's own handler on the path that ends the process, and
/// through [`die_from_signal`] by `pnpm-executor`'s relay when it is the
/// handler that ends the process.
pub fn remove_pending_temp_files() {
    ACTIVE_WALKS.fetch_add(1, Ordering::SeqCst);
    let mut next = HEAD.load(Ordering::Acquire);
    // SAFETY: every pointer in the list came from the `Box::leak` in
    // `push_entry` and is never freed, so it stays dereferenceable.
    while let Some(entry) = unsafe { next.as_ref() } {
        let path = entry.path.load(Ordering::SeqCst);
        // SAFETY: a non-null stored path came from `track_temp_file`, and
        // `ACTIVE_WALKS` keeps it from being freed until this walk ends.
        if let Some(stored) = unsafe { path.as_ref() } {
            unlink_stored(stored);
        }
        next = entry.next.load(Ordering::Acquire);
    }
    ACTIVE_WALKS.fetch_sub(1, Ordering::SeqCst);
}

/// Take the first free slot for `stored`, or extend the list with one. A
/// slot is claimed with a single compare-and-swap, so the handler either
/// still reads null or reads the published path, never a half-claimed slot.
fn claim_entry(stored: *mut StoredPath) -> &'static Entry {
    let mut next = HEAD.load(Ordering::Acquire);
    // SAFETY: as in `remove_pending_temp_files`, list entries are leaked and
    // stay dereferenceable for the life of the process.
    while let Some(entry) = unsafe { next.as_ref() } {
        if entry.path
            .compare_exchange(ptr::null_mut(), stored, Ordering::SeqCst, Ordering::Acquire)
            .is_ok()
        {
            return entry;
        }
        next = entry.next.load(Ordering::Acquire);
    }
    push_entry(stored)
}

/// Add an entry for `stored` at the head of the list. `path` is set before
/// the head swings, so a handler walking the list concurrently either
/// misses the new entry entirely or reads a complete one.
fn push_entry(stored: *mut StoredPath) -> &'static Entry {
    let entry: &'static Entry = Box::leak(Box::new(Entry {
        path: AtomicPtr::new(stored),
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

#[cfg(unix)]
fn store_path(path: &Path) -> Option<StoredPath> {
    use std::os::unix::ffi::OsStrExt as _;

    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

#[cfg(not(unix))]
fn store_path(path: &Path) -> Option<StoredPath> {
    Some(path.to_path_buf())
}

#[cfg(unix)]
fn unlink_stored(path: &StoredPath) {
    // SAFETY: `path` is a valid C string for the duration of the walk, and
    // `unlink` is async-signal-safe.
    unsafe {
        libc::unlink(path.as_ptr());
    }
}

#[cfg(not(unix))]
fn unlink_stored(path: &StoredPath) {
    let _ = std::fs::remove_file(path);
}

/// Install the handler that unlinks the pending temp files on interrupt.
/// [`track_temp_file`] installs it on first use. A module that installs its
/// own handler for the same signals calls this first, so the two
/// read-then-replace sequences cannot interleave and lose one handler.
#[cfg(unix)]
pub fn install_temp_file_cleanup() {
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
/// handler installed earlier may keep the process alive, and unlinking now
/// would pull temp files out from under writes that keep running.
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
        die_from_signal(signal);
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

/// Unlink the pending temp files, then end the process as `signal` would
/// have ended it without a handler. For a signal handler that decides the
/// process dies.
///
/// Async-signal-safe. The signal is unblocked first because a handler runs
/// with its own signal blocked: `raise` would otherwise leave it pending
/// until the handler returned, and the `_exit` below would report a plain
/// exit code where the caller expects death by a signal.
#[cfg(unix)]
pub fn die_from_signal(signal: libc::c_int) -> ! {
    remove_pending_temp_files();
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

/// Install the handler that unlinks the pending temp files on a console
/// interrupt. [`track_temp_file`] installs it on first use. Console
/// handlers run last-registered first, so a module that installs its own
/// handler after calling this sees each event before the cleanup does.
#[cfg(windows)]
pub fn install_temp_file_cleanup() {
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

/// Remove the pending temp files, then pass the event on to the default
/// termination. `pnpm-executor`'s relay, registered after this handler,
/// sees the event first and stops it here while children are running.
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
pub fn install_temp_file_cleanup() {}

#[cfg(test)]
mod tests;
