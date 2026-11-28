//! Temp files of in-flight atomic writes, unlinked when the process is
//! interrupted.
//!
//! An atomic write stages its content in a temp file next to the target and
//! publishes it by `rename`. A signal (`Ctrl+C` at the terminal, a service
//! manager's `SIGTERM`) takes the process down without running destructors,
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
//!
//! A signal is handled on whichever thread it lands on while writes go on
//! running on the others, so the cleanup that ends the process first closes
//! a gate that every write passes before it creates a temp file. A write
//! that finds the gate closed blocks until the process has ended. The
//! cleanup then waits for the writes already past the gate, so that each
//! has created and registered its temp file before the walk unlinks it.
//! Without the gate, a write could create its temp file after the walk had
//! unlinked the path, or register it in a slot the walk had already passed,
//! and the file stayed behind.

#![cfg_attr(
    not(unix),
    allow(
        rustdoc::broken_intra_doc_links,
        reason = "`die_from_signal` only exists on unix, so the links to it only resolve there"
    )
)]

use pnpm_signal_registry::SignalRegistry;
use std::{
    io,
    path::Path,
    sync::atomic::{
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
    time::Duration,
};

/// The registered path as the cleanup reads it: a C string on Unix, where
/// the reader is a signal handler and encoding one would allocate.
#[cfg(unix)]
type StoredPath = std::ffi::CString;

#[cfg(not(unix))]
type StoredPath = std::path::PathBuf;

static PENDING: PendingTempFiles = PendingTempFiles::new(EXIT_WAIT_LIMIT);

/// How long the cleanup that ends the process waits for the writes past the
/// gate. A creation is one `open`, so the wait normally ends within
/// microseconds. The limit covers a signal that lands on a thread mid
/// creation, which cannot finish while the handler runs. That temp file is
/// either registered and created, so the walk unlinks it, or never created.
const EXIT_WAIT_LIMIT: Duration = Duration::from_millis(100);

/// How long the cleanup sleeps between two checks of the writes past the
/// gate.
const EXIT_WAIT_STEP: Duration = Duration::from_millis(1);

/// The temp files of in-flight atomic writes, and the gate that stops new
/// ones once the process is ending.
struct PendingTempFiles {
    registry: SignalRegistry<StoredPath>,
    /// Closed by the cleanup that ends the process. No write creates a temp
    /// file once it is closed.
    exiting: AtomicBool,
    /// How many writes have passed the gate and not yet finished creating
    /// their temp file.
    creating: AtomicUsize,
    exit_wait_limit: Duration,
}

impl PendingTempFiles {
    const fn new(exit_wait_limit: Duration) -> Self {
        Self {
            registry: SignalRegistry::new(),
            exiting: AtomicBool::new(false),
            creating: AtomicUsize::new(0),
            exit_wait_limit,
        }
    }

    fn track<Created>(
        &'static self,
        path: &Path,
        create: impl FnOnce() -> io::Result<Created>,
    ) -> io::Result<(PendingTempFile, Created)> {
        let _creating = self.pass_gate();
        // Registered before the create, so a signal landing on this thread
        // between the two cannot leave the fresh temp file behind.
        let registration = store_path(path).map(|path| self.registry.register(path));
        // A failed create means the path was never ours. Returning drops the
        // registration before `_creating`, so the cleanup, which waits for
        // `_creating`, cannot unlink a file that belongs to someone else.
        let created = create()?;
        Ok((PendingTempFile { _registration: registration }, created))
    }

    /// Count this write among those creating a temp file, or block until
    /// the process ends if the gate is closed.
    ///
    /// The increment comes before the load, and the cleanup's store comes
    /// before its own load, all in `SeqCst`. Of the two loads, at least one
    /// sees the other side's write: either this write sees the gate closed,
    /// or the cleanup sees this write counted and waits for it.
    fn pass_gate(&self) -> Creating<'_> {
        self.creating.fetch_add(1, Ordering::SeqCst);
        if self.exiting.load(Ordering::SeqCst) {
            self.creating.fetch_sub(1, Ordering::SeqCst);
            block_until_exit();
        }
        Creating { count: &self.creating }
    }

    /// Close the gate, wait up to the limit for the writes past it, then
    /// unlink every registered temp file. For the path that ends the
    /// process, since no temp file is created afterwards.
    ///
    /// Async-signal-safe: the wait reads an atomic and sleeps, and
    /// `std::thread::sleep` is a `nanosleep` loop on Unix.
    #[cfg_attr(
        not(any(unix, windows, test)),
        expect(dead_code, reason = "only the Unix and Windows interrupt handlers end the process")
    )]
    fn remove_before_exit(&self) {
        self.exiting.store(true, Ordering::SeqCst);
        let mut remaining = self.exit_wait_limit;
        while self.creating.load(Ordering::SeqCst) != 0 && !remaining.is_zero() {
            std::thread::sleep(EXIT_WAIT_STEP);
            remaining = remaining.saturating_sub(EXIT_WAIT_STEP);
        }
        self.remove_all();
    }

    fn remove_all(&self) {
        self.registry.for_each(unlink_stored);
    }
}

/// Block the calling thread for the moment the process takes to end.
fn block_until_exit() -> ! {
    loop {
        std::thread::park();
    }
}

/// A write's place in the count of writes past the gate, given up when it
/// is dropped.
struct Creating<'files> {
    count: &'files AtomicUsize,
}

impl Drop for Creating<'_> {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A staged temp file's registration. Dropping it releases the slot: the
/// write has published or removed the temp file itself.
pub struct PendingTempFile {
    _registration: Option<pnpm_signal_registry::Registration<'static, StoredPath>>,
}

/// Register `path` as a temp file to unlink if the process is interrupted,
/// then create it with `create`. The registration lasts until the returned
/// guard is dropped. A failed `create` releases it at once, since the path
/// then belongs to whoever created it.
///
/// Once an interrupt has started ending the process, this blocks until the
/// process has ended rather than create a temp file the cleanup would miss.
pub fn track_temp_file<Created>(
    path: &Path,
    create: impl FnOnce() -> io::Result<Created>,
) -> io::Result<(PendingTempFile, Created)> {
    install_temp_file_cleanup();
    PENDING.track(path, create)
}

/// Unlink every temp file still registered, leaving writes free to create
/// more. The cleanup that ends the process goes through [`die_from_signal`]
/// instead, which first stops them.
///
/// Async-signal-safe: the walk takes no lock and allocates nothing, and
/// `unlink` is safe to call from a handler.
pub fn remove_pending_temp_files() {
    PENDING.remove_all();
}

#[cfg(unix)]
fn store_path(path: &Path) -> Option<StoredPath> {
    use std::os::unix::ffi::OsStrExt as _;

    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

#[cfg(not(unix))]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::needless_borrowed_parameters,
        reason = "the `cfg(unix)` twin encodes this parameter into a `CString` rather than owning it, and both must offer one signature to the shared call site"
    )
)]
fn store_path(path: &Path) -> Option<StoredPath> {
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

/// Stop writes from creating temp files, unlink the pending ones, then end
/// the process as `signal` would have ended it without a handler, without
/// dumping pnpm's own core. For a signal handler or child-status exit that
/// decides the process dies.
///
/// Async-signal-safe.
#[cfg(unix)]
pub fn die_from_signal(signal: i32) -> ! {
    PENDING.remove_before_exit();
    // A core-dumping signal re-raised here (`SIGABRT`, `SIGSEGV`,
    // `SIGQUIT`, ...) is a child's status being reported as pnpm's own
    // exit. The child already had its chance to dump core, and a dump here
    // would record a crash of pnpm in `systemd-coredump` or overwrite the
    // child's own `core` file
    // ([pnpm/pnpm#17204](https://github.com/pnpm/pnpm/issues/17204)).
    pnpm_ffi_process::suppress_core_dump();
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

/// Stop writes from creating temp files and remove the pending ones, then
/// pass the event on to the default termination. `pnpm-executor`'s relay,
/// registered after this handler, sees the event first and stops it there
/// while children are running.
#[cfg(windows)]
fn clean_temp_files(_interrupt: pnpm_ffi_signal::Interrupt) -> pnpm_ffi_signal::Handled {
    PENDING.remove_before_exit();
    pnpm_ffi_signal::Handled::No
}

#[cfg(not(any(unix, windows)))]
pub fn install_temp_file_cleanup() {}

#[cfg(test)]
mod tests;
