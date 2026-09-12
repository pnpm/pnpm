//! Waiting for a script child that the terminal interrupted alongside pnpm.
//!
//! `Ctrl+C` raises `SIGINT` in every process of the terminal's foreground
//! group, so it reaches pnpm and the script pnpm started at the same
//! moment. Under the default disposition pnpm dies first and the shell
//! draws its prompt while the script is still shutting down, so whatever
//! the script writes next — or whatever the terminal answers a query the
//! script left open — lands on that prompt. pnpm instead relays the signal
//! to the children it started, waits for them, and ends the way they did.
//!
//! Every child spawned through [`crate::spawn_child`] registers here for
//! the length of its run. The relay escalates so a script cannot trap the
//! terminal: the first interrupt is passed on as it arrived, the second
//! becomes `SIGTERM`, and the third stops the waiting and lets the signal
//! take pnpm down.

use crate::ScriptExit;
use std::{
    ptr,
    sync::{
        Once,
        atomic::{AtomicI32, AtomicPtr, AtomicUsize, Ordering},
    },
};

/// The number of interrupts pnpm relays to a child before it stops
/// waiting for that child. Once every child it reaches has had them, the
/// next interrupt ends pnpm instead.
const RELAYED_INTERRUPTS: usize = 2;

/// One running child: the process to signal, negated to address the whole
/// process group when the child leads one, or `0` when the entry is free.
///
/// Entries form a list the signal handler walks, so they are leaked rather
/// than freed, and the entry a finished child leaves behind is reused by
/// the next one. The handler then only reads atomics, which is
/// async-signal-safe, while the list itself has no fixed size: a table
/// with one would stop covering children once a wide `--parallel` run
/// filled it, and those children would survive the interrupt.
struct RelayEntry {
    target: AtomicI32,
    /// Interrupts relayed to the child holding this entry. Counting per
    /// child rather than per process is what gives a script that starts
    /// later its own plain first interrupt, while still escalating for
    /// one that sits through them.
    relays: AtomicUsize,
    next: AtomicPtr<RelayEntry>,
}

static RELAY_HEAD: AtomicPtr<RelayEntry> = AtomicPtr::new(ptr::null_mut());

/// A claim in progress: the entry belongs to a spawn that has not
/// published its child yet. No process or process group is ever
/// `i32::MIN`, so it cannot collide with a real target.
const CLAIMING: i32 = i32::MIN;

/// A child's registration in the relay, released when dropped.
pub(crate) struct SignalRelay {
    entry: Option<&'static RelayEntry>,
}

impl Drop for SignalRelay {
    fn drop(&mut self) {
        if let Some(entry) = self.entry {
            entry.target.store(0, Ordering::Release);
        }
    }
}

/// Relay the terminal signals pnpm receives to `pid` until the returned
/// guard is dropped.
///
/// `own_process_group` says the child leads a process group of its own
/// (see [`crate::ProcessTracker`]); the whole group is then signalled,
/// because the terminal's signals never reach it.
pub(crate) fn relay_to_child(pid: u32, own_process_group: bool) -> SignalRelay {
    install_handler();
    let Ok(pid) = i32::try_from(pid) else {
        return SignalRelay { entry: None };
    };
    let target = if own_process_group { -pid } else { pid };
    SignalRelay { entry: Some(claim_entry(target)) }
}

/// Take the first free entry for `target`, or extend the list with one.
fn claim_entry(target: i32) -> &'static RelayEntry {
    let mut next = RELAY_HEAD.load(Ordering::Acquire);
    // SAFETY: every pointer in the list came from the `Box::leak` in
    // `push_entry` and is never freed, so it stays dereferenceable.
    while let Some(entry) = unsafe { next.as_ref() } {
        // The plain read first keeps a scan past occupied entries to
        // shared loads: a failing `compare_exchange` would still take
        // each line exclusively, which is what a wide parallel run would
        // pay for on every spawn.
        if entry.target.load(Ordering::Relaxed) == 0
            && entry
                .target
                .compare_exchange(0, CLAIMING, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            // The entry is this thread's alone now, so the count can be
            // cleared before the child is published. Clearing it before
            // taking the entry would let a thread that lost the race wipe
            // the escalation of whichever child won it.
            entry.relays.store(0, Ordering::Relaxed);
            entry.target.store(target, Ordering::Release);
            return entry;
        }
        next = entry.next.load(Ordering::Acquire);
    }
    push_entry(target)
}

/// Add an entry for `target` at the head of the list. `next` is set before
/// the head swings, so a handler walking the list concurrently either
/// misses the new entry entirely or reads a complete one.
fn push_entry(target: i32) -> &'static RelayEntry {
    let entry: &'static RelayEntry = Box::leak(Box::new(RelayEntry {
        target: AtomicI32::new(target),
        relays: AtomicUsize::new(0),
        next: AtomicPtr::new(ptr::null_mut()),
    }));
    let mut head = RELAY_HEAD.load(Ordering::Acquire);
    loop {
        entry.next.store(head, Ordering::Release);
        match RELAY_HEAD.compare_exchange_weak(
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

/// Call `visit` with every registered child and report whether there was
/// one. The walk allocates nothing, so a signal handler can run it.
fn visit_targets(mut visit: impl FnMut(&RelayEntry, i32)) -> bool {
    let mut visited = false;
    let mut next = RELAY_HEAD.load(Ordering::Acquire);
    // SAFETY: as in `claim_entry`, list entries are leaked and stay
    // dereferenceable for the life of the process.
    while let Some(entry) = unsafe { next.as_ref() } {
        let target = entry.target.load(Ordering::Acquire);
        if target != 0 && target != CLAIMING {
            visited = true;
            visit(entry, target);
        }
        next = entry.next.load(Ordering::Acquire);
    }
    visited
}

/// End pnpm the way `exit` ended.
///
/// A child killed by a signal makes pnpm re-raise that signal on itself,
/// so the shell reports an interrupted command rather than a plain
/// non-zero exit. Anything else exits with the child's code.
#[cfg(unix)]
#[expect(clippy::exit, reason = "the command's exit code is the script's")]
pub fn exit_like(exit: ScriptExit) -> ! {
    use std::os::unix::process::ExitStatusExt;

    if let ScriptExit::Process(status) = exit
        && let Some(signal) = status.signal()
    {
        die_from(signal);
    }
    std::process::exit(exit.code().unwrap_or(1));
}

/// End pnpm the way `exit` ended. Windows has no signals, so that is the
/// script's exit code.
#[cfg(not(unix))]
#[expect(clippy::exit, reason = "the command's exit code is the script's")]
pub fn exit_like(exit: ScriptExit) -> ! {
    std::process::exit(exit.code().unwrap_or(1));
}

#[cfg(unix)]
fn install_handler() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            install_handler_for(signal);
        }
    });
}

#[cfg(unix)]
fn install_handler_for(signal: libc::c_int) {
    // SAFETY: both calls take stack locals that outlive them, and a null
    // `act` only reads the current disposition back.
    unsafe {
        let mut previous: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(signal, std::ptr::null(), &raw mut previous) != 0 {
            return;
        }
        // A signal pnpm was started with ignored stays ignored, so pnpm
        // under `nohup` or in a background job keeps that disposition,
        // which its children inherit across `exec`.
        if previous.sa_sigaction == libc::SIG_IGN {
            return;
        }
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = relay_signal as *const () as usize;
        // `SA_RESTART` keeps the `waitpid` and the output pumps running
        // underneath from failing with `EINTR` when the relay fires.
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&raw mut action.sa_mask);
        libc::sigaction(signal, &raw const action, std::ptr::null_mut());
    }
}

/// Pass `signal` on to the running children, or end pnpm when there is no
/// longer anything to wait for.
///
/// Everything it calls is async-signal-safe: atomic loads, `kill`,
/// `signal`, `raise`, and `_exit`.
#[cfg(unix)]
extern "C" fn relay_signal(signal: libc::c_int) {
    let mut still_listening = false;
    let reached = visit_targets(|entry, target| {
        // pnpm reaps a child before releasing its entry, so an entry that
        // has turned over since the walk read it names a process pnpm no
        // longer owns. Leave it alone, and leave the child that took the
        // entry its own first interrupt.
        if entry.target.load(Ordering::Acquire) != target {
            return;
        }
        let step = entry.relays.fetch_add(1, Ordering::Relaxed);
        if step >= RELAYED_INTERRUPTS {
            return;
        }
        still_listening = true;
        let relayed = if step == 0 { signal } else { libc::SIGTERM };
        // SAFETY: `target` is a child pnpm spawned, or that child's
        // process group. `ESRCH` from one that exited concurrently is
        // harmless.
        unsafe {
            libc::kill(target, relayed);
        }
    });
    // Nothing left to wait for, or every child has had its interrupt and
    // its `SIGTERM` and sat through both.
    if !reached || !still_listening {
        die_from(signal);
    }
}

/// End pnpm as `signal` would have ended it without this module.
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
        libc::sigprocmask(libc::SIG_UNBLOCK, &raw const unblocked, std::ptr::null_mut());
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
            SetConsoleCtrlHandler(Some(relay_console_event), 1);
        }
    });
}

/// Report a console interrupt as handled while children are still
/// running, which is what keeps pnpm alive. The console delivers the same
/// event to every process attached to it, so the children already have
/// it and nothing needs relaying.
///
/// Returning `FALSE` passes the event on to the default handler, which
/// terminates pnpm.
#[cfg(windows)]
unsafe extern "system" fn relay_console_event(event: u32) -> windows_sys::core::BOOL {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};

    if !matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        return 0;
    }
    let mut still_listening = false;
    visit_targets(|entry, _| {
        if entry.relays.fetch_add(1, Ordering::Relaxed) < RELAYED_INTERRUPTS {
            still_listening = true;
        }
    });
    windows_sys::core::BOOL::from(still_listening)
}

#[cfg(not(any(unix, windows)))]
fn install_handler() {}
