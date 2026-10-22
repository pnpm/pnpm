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
//!
//! Windows has no signal to pass on: the console's control event reaches
//! every attached process at once. What a child needs ending there is the
//! case that never ends on its own — a `cmd` hosting a batch script, which
//! answers the event with a "Terminate batch job (Y/N)?" prompt and waits
//! on the answer forever. `cmd` asks only once the command it ran has
//! returned, so a `cmd` left without child processes of its own for a
//! grace after the interrupt is ended by pnpm, while one whose script is
//! still shutting down is waited for
//! ([pnpm/pnpm#14860](https://github.com/pnpm/pnpm/issues/14860)).
//!
//! A child that shares pnpm's process group has the terminal's interrupt
//! already, so pnpm does not pass that one on: a second `SIGINT` would
//! end a script that handles the signal once and then leaves the default
//! action in place, before its shutdown has finished
//! ([pnpm/pnpm#7374](https://github.com/pnpm/pnpm/issues/7374)). Only a
//! signal aimed at pnpm alone, as `kill` does, is relayed to that child.
//!
//! Without a controlling terminal, under a service manager or in a
//! container, `kill` is the only way a signal reaches pnpm, and the shell
//! running the script may not pass it on: a `sh` that stays the script's
//! parent dies from `SIGTERM` at once and holds a `SIGINT` until its child
//! exits, which it never does unsignalled. Each child then gets a process
//! group of its own, the relay signals the group, and pnpm waits until
//! nothing in the group is still running after a relayed signal, so the
//! script finishes shutting down before pnpm ends.

use crate::ScriptExit;
use pnpm_signal_registry::SignalList;
#[cfg(unix)]
use std::sync::Once;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

/// The number of interrupts pnpm relays to a child before it stops
/// waiting for that child. Once every child it reaches has had them, the
/// next interrupt ends pnpm instead.
const RELAYED_INTERRUPTS: usize = 2;

/// Counted up by the signal and console handlers, read through
/// [`interrupt_count`].
static INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

/// How many interrupts and terminations have reached pnpm so far: signals
/// pnpm handles, or console control events on Windows. `pnpm run` reads
/// it before and after a script, so it can tell a script the user ended
/// from one that failed on its own after an earlier, handled interrupt.
/// The user ended the former, so its status is not a lifecycle failure to
/// report, and pnpm ends with it all the same (pnpm/pnpm#16579).
#[must_use]
pub fn interrupt_count() -> usize {
    INTERRUPTS.load(Ordering::Relaxed)
}

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
    /// A handle that can end and await the child, opened at registration
    /// while the child is provably alive. The handle pins the process
    /// object, so the pid in `target` can never come to name a recycled
    /// process between this entry's release and a late termination.
    ///
    /// The lock guards the handle through its lifecycle: a termination
    /// holds it for the length of its call, a release for the target
    /// clear, and a claim for the handle's retirement. A handle is
    /// therefore dropped only when no termination can be using it. A lock
    /// is allowed here because Windows delivers a console event on a
    /// thread of its own rather than in a signal handler.
    #[cfg(windows)]
    handle: std::sync::Mutex<Option<pnpm_ffi_process::ProcessHandle>>,
    /// Interrupts relayed to the child holding this entry. Counting per
    /// child rather than per process is what gives a script that starts
    /// later its own plain first interrupt, while still escalating for
    /// one that sits through them.
    relays: AtomicUsize,
}

static RELAY_ENTRIES: SignalList<RelayEntry> = SignalList::new();

/// A claim in progress: the entry belongs to a spawn that has not
/// published its child yet. No process or process group is ever
/// `i32::MIN`, so it cannot collide with a real target.
const CLAIMING: i32 = i32::MIN;

/// A child's registration in the relay, released when dropped.
pub(crate) struct SignalRelay {
    entry: Option<&'static RelayEntry>,
}

impl SignalRelay {
    /// Whether pnpm has relayed a signal to this child.
    pub(crate) fn relayed(&self) -> bool {
        self.entry.is_some_and(|entry| entry.relays.load(Ordering::Relaxed) > 0)
    }
}

impl Drop for SignalRelay {
    fn drop(&mut self) {
        if let Some(entry) = self.entry {
            #[cfg(windows)]
            let _handle = entry.handle.lock().expect("relay handle lock is not poisoned");
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
    #[cfg(windows)]
    let handle = open_child_handle(pid);
    SignalRelay {
        entry: Some(claim_entry(
            target,
            #[cfg(windows)]
            handle,
        )),
    }
}

/// Open a handle to end and await the child while it is provably alive:
/// pnpm still holds its own handle for a child it just spawned, so the
/// process object — and its pid — cannot have been recycled.
///
/// `None` only means the child cannot be ended through a handle later,
/// which the termination path skips.
#[cfg(windows)]
fn open_child_handle(pid: i32) -> Option<pnpm_ffi_process::ProcessHandle> {
    pnpm_ffi_process::ProcessHandle::open(u32::try_from(pid).ok()?)
}

/// Take the first free entry for `target`, or extend the list with one.
fn claim_entry(
    target: i32,
    #[cfg(windows)] handle: Option<pnpm_ffi_process::ProcessHandle>,
) -> &'static RelayEntry {
    #[cfg(windows)]
    let handle = std::cell::Cell::new(handle);
    RELAY_ENTRIES.claim_or_push(
        |entry| {
            // The plain read first keeps a scan past occupied entries to
            // shared loads: a failing `compare_exchange` would still take
            // each line exclusively, which is what a wide parallel run
            // would pay for on every spawn.
            if entry.target.load(Ordering::Relaxed) != 0
                || entry.target
                    .compare_exchange(0, CLAIMING, Ordering::AcqRel, Ordering::Relaxed)
                    .is_err()
            {
                return false;
            }
            // The entry is this thread's alone now, so the count can be
            // cleared before the child is published. Clearing it before
            // taking the entry would let a thread that lost the race wipe
            // the escalation of whichever child won it.
            entry.relays.store(0, Ordering::Relaxed);
            // The handle the previous child left is dropped only now, with
            // the entry unpublished and the lock held, so no termination
            // can be using it.
            #[cfg(windows)]
            {
                *entry.handle.lock().expect("relay handle lock is not poisoned") = handle.take();
            }
            entry.target.store(target, Ordering::Release);
            true
        },
        || RelayEntry {
            target: AtomicI32::new(target),
            #[cfg(windows)]
            handle: std::sync::Mutex::new(handle.take()),
            relays: AtomicUsize::new(0),
        },
    )
}

/// Call `visit` with every registered child and report whether there was
/// one. The walk allocates nothing, so a signal handler can run it.
fn visit_targets(mut visit: impl FnMut(&'static RelayEntry, i32)) -> bool {
    let mut visited = false;
    RELAY_ENTRIES.for_each(|entry| {
        let target = entry.target.load(Ordering::Acquire);
        if target != 0 && target != CLAIMING {
            visited = true;
            visit(entry, target);
        }
    });
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
        pnpm_fs::die_from_signal(signal);
    }
    std::process::exit(exit.code().unwrap_or(1));
}

/// End pnpm the way `exit` ended. Windows has no signals, so that is the
/// script's exit code.
#[cfg(not(unix))]
#[expect(clippy::exit, reason = "the command's exit code is the script's")]
pub fn exit_like(exit: ScriptExit) -> ! {
    #[cfg(target_family = "wasm")]
    if let ScriptExit::Process(status) = exit
        && let Some(signal) = status.signal()
    {
        std::process::exit(128 + signal);
    }
    std::process::exit(exit.code().unwrap_or(1));
}

#[cfg(unix)]
fn install_handler() {
    pnpm_fs::install_temp_file_cleanup();
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        pnpm_ffi_signal::install(
            &[
                pnpm_ffi_signal::Interrupt::Interrupt,
                pnpm_ffi_signal::Interrupt::Terminate,
                pnpm_ffi_signal::Interrupt::Hangup,
            ],
            relay_signal,
        );
    });
}

/// Pass the interrupt on to the running children, or end pnpm when there is
/// no longer anything to wait for.
///
/// Everything it calls is async-signal-safe: atomic loads and updates,
/// `kill`, `signal`, `raise`, and `_exit`. The disposition it replaced is
/// not passed on: a relay that keeps pnpm running must not let the default
/// end it, and one that decides pnpm dies goes through
/// `die_from_signal`, which ends the process itself.
#[cfg(unix)]
fn relay_signal(interrupt: pnpm_ffi_signal::Interrupt, _replaced: pnpm_ffi_signal::Replaced) {
    let signal = signal_number(interrupt);
    INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    let shared_with_group =
        interrupt == pnpm_ffi_signal::Interrupt::Interrupt && pnpm_ffi_signal::holds_the_terminal();
    let mut still_listening = false;
    let reached = visit_targets(|entry, target| {
        still_listening |= relay_to(entry, target, signal, shared_with_group);
    });
    // Nothing left to wait for, or every child has had its interrupt and
    // its `SIGTERM` and sat through both.
    if !reached || !still_listening {
        pnpm_fs::die_from_signal(signal);
    }
}

#[cfg(unix)]
fn signal_number(interrupt: pnpm_ffi_signal::Interrupt) -> libc::c_int {
    match interrupt {
        pnpm_ffi_signal::Interrupt::Interrupt => libc::SIGINT,
        pnpm_ffi_signal::Interrupt::Terminate => libc::SIGTERM,
        pnpm_ffi_signal::Interrupt::Hangup => libc::SIGHUP,
    }
}

/// Relay `signal`, or the escalation the child holding `entry` has
/// reached, and report whether pnpm keeps waiting for that child.
///
/// `shared_with_group` says the terminal delivered `signal` to pnpm's
/// whole process group. A positive target is a child in that group, so
/// it has the signal already and only the escalation is passed on.
#[cfg(unix)]
fn relay_to(entry: &RelayEntry, target: i32, signal: libc::c_int, shared_with_group: bool) -> bool {
    // pnpm reaps a child before releasing its entry, so an entry that
    // has turned over since the walk read it names a process pnpm no
    // longer owns. Leave it alone, and leave the child that took the
    // entry its own first interrupt.
    if entry.target.load(Ordering::Acquire) != target {
        return false;
    }
    let step = entry.relays.fetch_add(1, Ordering::Relaxed);
    if step >= RELAYED_INTERRUPTS {
        return false;
    }
    if step == 0 && target > 0 && shared_with_group {
        return true;
    }
    let relayed = if step == 0 { signal } else { libc::SIGTERM };
    // A negative target names the process group the child leads, which is
    // the form `relay_to_child` stores for a child that leads one.
    let aimed_at = if target < 0 {
        pnpm_ffi_signal::Target::Group(target.unsigned_abs())
    } else {
        pnpm_ffi_signal::Target::Process(target.unsigned_abs())
    };
    pnpm_ffi_signal::signal_target(aimed_at, relayed);
    true
}

/// Whether pnpm has a controlling terminal. Without one, only `kill` can
/// signal pnpm, and nothing signals its children but pnpm.
///
/// Async-signal-safe.
#[cfg(unix)]
pub(crate) fn has_controlling_terminal() -> bool {
    pnpm_ffi_signal::has_controlling_terminal()
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::install_handler;

#[cfg(target_family = "wasm")]
mod wasm;
#[cfg(target_family = "wasm")]
use wasm::install_handler;

#[cfg(not(any(unix, windows, target_family = "wasm")))]
fn install_handler() {}
