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
//! This handler passes the signal on to whatever disposition it replaced
//! for the same reason, unlinking only before the one that ends the
//! process.

#![cfg_attr(
    not(unix),
    allow(
        rustdoc::broken_intra_doc_links,
        reason = "`die_from_signal` only exists on unix, so the links to it only resolve there"
    )
)]

use pnpm_signal_registry::SignalRegistry;

/// The registered path as the cleanup reads it: a C string on Unix, where
/// the reader is a signal handler and encoding one would allocate.
#[cfg(unix)]
type StoredPath = std::ffi::CString;

#[cfg(not(unix))]
type StoredPath = std::path::PathBuf;

static PENDING: SignalRegistry<StoredPath> = SignalRegistry::new();

/// A staged temp file's registration. Dropping it releases the slot: the
/// write has published or removed the temp file itself.
pub struct PendingTempFile {
    _registration: Option<pnpm_signal_registry::Registration<'static, StoredPath>>,
}

/// Register `path` as a temp file to unlink if the process is interrupted,
/// until the returned guard is dropped.
#[must_use]
pub fn track_temp_file(path: &std::path::Path) -> PendingTempFile {
    install_temp_file_cleanup();
    PendingTempFile { _registration: store_path(path).map(|path| PENDING.register(path)) }
}

/// Unlink every temp file still registered.
///
/// Async-signal-safe: the walk takes no lock and allocates nothing, and
/// `unlink` is safe to call from a handler. Called from this module's own
/// handler on the path that ends the process, and through
/// [`die_from_signal`] by `pnpm-executor`'s relay when that is the handler
/// which ends it.
pub fn remove_pending_temp_files() {
    PENDING.for_each(unlink_stored);
}

#[cfg(unix)]
fn store_path(path: &std::path::Path) -> Option<StoredPath> {
    use std::os::unix::ffi::OsStrExt as _;

    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

#[cfg(not(unix))]
fn store_path(path: &std::path::Path) -> Option<StoredPath> {
    Some(path.to_path_buf())
}

#[cfg(unix)]
fn unlink_stored(path: &StoredPath) {
    pnpm_ffi_fs::unlink_without_allocating(path);
}

#[cfg(not(unix))]
fn unlink_stored(path: &StoredPath) {
    let _ = std::fs::remove_file(path);
}

/// The terminal and service-manager interrupts that end pnpm without
/// running destructors.
#[cfg(unix)]
const INTERRUPTS: [pnpm_ffi_signal::Interrupt; 3] = [
    pnpm_ffi_signal::Interrupt::Interrupt,
    pnpm_ffi_signal::Interrupt::Terminate,
    pnpm_ffi_signal::Interrupt::Hangup,
];

/// Install the handler that unlinks the pending temp files on interrupt.
/// [`track_temp_file`] installs it on first use. A module that installs its
/// own handler for the same signals calls this first, so the two
/// read-then-replace sequences cannot interleave and lose one handler.
#[cfg(unix)]
pub fn install_temp_file_cleanup() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| pnpm_ffi_signal::install(&INTERRUPTS, clean_temp_files));
}

/// Pass the signal on to the disposition this handler replaced, unlinking
/// the pending temp files only on the path that ends the process: the
/// default dies by the signal, so it gets the cleanup first, while a
/// handler installed earlier may keep the process alive, and unlinking now
/// would pull temp files out from under writes that keep running.
#[cfg(unix)]
fn clean_temp_files(interrupt: pnpm_ffi_signal::Interrupt, replaced: pnpm_ffi_signal::Replaced) {
    if replaced.ends_the_process() {
        die_from_signal(signal_number(interrupt));
    }
    replaced.pass_on();
}

#[cfg(unix)]
fn signal_number(interrupt: pnpm_ffi_signal::Interrupt) -> i32 {
    match interrupt {
        pnpm_ffi_signal::Interrupt::Interrupt => libc::SIGINT,
        pnpm_ffi_signal::Interrupt::Terminate => libc::SIGTERM,
        pnpm_ffi_signal::Interrupt::Hangup => libc::SIGHUP,
    }
}

/// Unlink the pending temp files, then end the process as `signal` would
/// have ended it without a handler. For a signal handler that decides the
/// process dies.
///
/// Async-signal-safe.
#[cfg(unix)]
pub fn die_from_signal(signal: i32) -> ! {
    remove_pending_temp_files();
    pnpm_ffi_signal::die_from(signal)
}

/// Install the handler that unlinks the pending temp files on a console
/// interrupt. [`track_temp_file`] installs it on first use. Console
/// handlers run last-registered first, so a module that installs its own
/// handler after calling this sees each event before the cleanup does.
#[cfg(windows)]
pub fn install_temp_file_cleanup() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| pnpm_ffi_signal::on_console_event(clean_temp_files));
}

/// Remove the pending temp files, then pass the event on to the default
/// termination. `pnpm-executor`'s relay, registered after this handler,
/// sees the event first and stops it there while children are running.
#[cfg(windows)]
fn clean_temp_files(_interrupt: pnpm_ffi_signal::Interrupt) -> pnpm_ffi_signal::Handled {
    remove_pending_temp_files();
    pnpm_ffi_signal::Handled::No
}

#[cfg(not(any(unix, windows)))]
pub fn install_temp_file_cleanup() {}

#[cfg(test)]
mod tests;
