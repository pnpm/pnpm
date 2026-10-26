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
#[cfg(unix)]
use std::sync::Once;
#[cfg(windows)]
use std::sync::atomic::AtomicBool;
use std::{
    ptr,
    sync::atomic::{AtomicI32, AtomicPtr, AtomicUsize, Ordering},
};

/// The number of interrupts pnpm relays to a child before it stops
/// waiting for that child. Once every child it reaches has had them, the
/// next interrupt ends pnpm instead.
const RELAYED_INTERRUPTS: usize = 2;

/// Counted up by the signal and console handlers, read through
/// [`interrupt_count`].
static INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

/// The signal the handler took last, published before it counts it in
/// [`INTERRUPTS`], so a thread that sees the count change sees the signal
/// behind it.
#[cfg(unix)]
static LAST_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// Children pnpm has spawned that have not joined the relay yet. The
/// handler passes a signal on to the children that have, so these are
/// children it cannot reach, and ending pnpm would leave them behind.
static SPAWNING: AtomicUsize = AtomicUsize::new(0);

/// Signals pnpm took while it had a child being spawned, counted from the
/// first such child. A spawn in flight absorbs no more of them than a
/// child in the relay does, so spawning cannot hold the terminal.
static SPAWNING_INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

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
    /// Guards `handle` through its lifecycle: a termination holds it for
    /// the length of its `TerminateProcess` call, a release for the target
    /// clear, and a claim for the handle's retirement. A handle is
    /// therefore closed only when no termination can be using it.
    #[cfg(windows)]
    handle_lock: AtomicBool,
    /// A `PROCESS_TERMINATE` handle for the child, opened at registration
    /// while the child is provably alive. The handle pins the process
    /// object, so the pid in `target` can never come to name a recycled
    /// process between this entry's release and a late termination.
    #[cfg(windows)]
    handle: AtomicPtr<std::ffi::c_void>,
    /// Interrupts relayed to the child holding this entry. Counting per
    /// child rather than per process is what gives a script that starts
    /// later its own plain first interrupt, while still escalating for
    /// one that sits through them.
    relays: AtomicUsize,
    next: AtomicPtr<RelayEntry>,
}

#[cfg(windows)]
impl RelayEntry {
    fn lock_handle(&self) {
        while self.handle_lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
    }

    fn unlock_handle(&self) {
        self.handle_lock.store(false, Ordering::Release);
    }
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

impl SignalRelay {
    /// Whether pnpm has relayed a signal to this child.
    pub(crate) fn relayed(&self) -> bool {
        self.entry.is_some_and(|entry| entry.relays.load(Ordering::Relaxed) > 0)
    }
}

/// Pass on to `relay`'s child the signal pnpm took while it was being
/// spawned.
///
/// Taking the first relay here is what keeps the handler from taking it as
/// well, so the child is never given one interrupt twice.
///
/// `missed_the_child` says pnpm took the signal before the child existed,
/// so no process group the child is in carried it and the relay owes it
/// the signal whatever group that is.
#[cfg(unix)]
fn relay_late(relay: &SignalRelay, missed_the_child: bool) {
    let Some(entry) = relay.entry else { return };
    if entry.relays.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed).is_err() {
        return;
    }
    let signal = LAST_SIGNAL.load(Ordering::Relaxed);
    let target = entry.target.load(Ordering::Acquire);
    // A child that was already in pnpm's own process group was given the
    // terminal's interrupt by the kernel, which is how [`relay_to`] reads
    // it too.
    if !missed_the_child && target > 0 && signal == libc::SIGINT && holds_the_terminal() {
        return;
    }
    // SAFETY: `target` is the child this relay holds, or that child's
    // process group, and the relay holds the entry for as long as pnpm
    // waits for the child. `ESRCH` from one that exited concurrently is
    // harmless.
    unsafe {
        libc::kill(target, signal);
    }
}

/// The console delivers its event to every process attached to it, so a
/// child that joined the relay late has the event already.
#[cfg(not(unix))]
fn relay_late(_relay: &SignalRelay, _missed_the_child: bool) {}

/// A child pnpm has spawned that has not joined the relay yet.
///
/// pnpm waits for every child it spawns, so a signal that arrives before
/// the child joins must still reach it: nothing else would end that child,
/// and pnpm would wait for it for as long as it ran. The handler therefore
/// counts a child being spawned as one more to wait for, and
/// [`Self::joined`] passes on what the handler could not.
pub(crate) struct SpawningChild {
    /// [`INTERRUPTS`] as it stood before the child was spawned.
    before_spawn: usize,
    /// [`INTERRUPTS`] as it stood once the child existed.
    once_spawned: usize,
}

/// Account for a child pnpm is about to spawn, and take the signals that
/// would otherwise end pnpm with their default disposition.
///
/// Call it before the spawn: a signal taken from here on is one the
/// handler cannot pass on, and one pnpm must outlive, or the child would
/// be left behind by the very spawn that created it.
pub(crate) fn spawning_child() -> SpawningChild {
    install_handler();
    if SPAWNING.fetch_add(1, Ordering::Release) == 0 {
        SPAWNING_INTERRUPTS.store(0, Ordering::Release);
    }
    let taken = INTERRUPTS.load(Ordering::Acquire);
    SpawningChild { before_spawn: taken, once_spawned: taken }
}

impl SpawningChild {
    /// Note that the child now exists, so a signal counted from here on
    /// reached whatever process group the child is already in.
    pub(crate) fn spawned(&mut self) {
        self.once_spawned = INTERRUPTS.load(Ordering::Acquire);
    }

    /// Hand the child over to `relay`, passing on whatever pnpm took while
    /// it was being spawned.
    pub(crate) fn joined(self, relay: &SignalRelay) {
        if INTERRUPTS.load(Ordering::Acquire) == self.before_spawn {
            return;
        }
        relay_late(relay, self.once_spawned != self.before_spawn);
    }
}

impl Drop for SpawningChild {
    fn drop(&mut self) {
        SPAWNING.fetch_sub(1, Ordering::Release);
    }
}

impl Drop for SignalRelay {
    fn drop(&mut self) {
        if let Some(entry) = self.entry {
            #[cfg(windows)]
            entry.lock_handle();
            entry.target.store(0, Ordering::Release);
            #[cfg(windows)]
            entry.unlock_handle();
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
#[cfg(windows)]
fn open_child_handle(pid: i32) -> *mut std::ffi::c_void {
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    // SAFETY: a plain query by pid; a null result only means the child
    // cannot be ended through it later, which the termination path skips.
    unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid as u32) }
}

/// Take the first free entry for `target`, or extend the list with one.
fn claim_entry(target: i32, #[cfg(windows)] handle: *mut std::ffi::c_void) -> &'static RelayEntry {
    let mut next = RELAY_HEAD.load(Ordering::Acquire);
    // SAFETY: every pointer in the list came from the `Box::leak` in
    // `push_entry` and is never freed, so it stays dereferenceable.
    while let Some(entry) = unsafe { next.as_ref() } {
        // The plain read first keeps a scan past occupied entries to
        // shared loads: a failing `compare_exchange` would still take
        // each line exclusively, which is what a wide parallel run would
        // pay for on every spawn.
        if entry.target.load(Ordering::Relaxed) == 0
            && entry.target
                .compare_exchange(0, CLAIMING, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            #[cfg(windows)]
            entry.lock_handle();
            // The entry is this thread's alone now, so the count can be
            // cleared before the child is published. Clearing it before
            // taking the entry would let a thread that lost the race wipe
            // the escalation of whichever child won it.
            entry.relays.store(0, Ordering::Relaxed);
            // The handle the previous child left is retired only now, with
            // the entry unpublished and the lock held, so no termination
            // can be using it.
            #[cfg(windows)]
            close_retired_handle(entry.handle.swap(handle, Ordering::Relaxed));
            #[cfg(windows)]
            entry.unlock_handle();
            entry.target.store(target, Ordering::Release);
            return entry;
        }
        next = entry.next.load(Ordering::Acquire);
    }
    push_entry(
        target,
        #[cfg(windows)]
        handle,
    )
}

/// Add an entry for `target` at the head of the list. `next` is set before
/// the head swings, so a handler walking the list concurrently either
/// misses the new entry entirely or reads a complete one.
fn push_entry(target: i32, #[cfg(windows)] handle: *mut std::ffi::c_void) -> &'static RelayEntry {
    let entry: &'static RelayEntry = Box::leak(Box::new(RelayEntry {
        target: AtomicI32::new(target),
        #[cfg(windows)]
        handle_lock: AtomicBool::new(false),
        #[cfg(windows)]
        handle: AtomicPtr::new(handle),
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
fn visit_targets(mut visit: impl FnMut(&'static RelayEntry, i32)) -> bool {
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
/// Everything it calls is async-signal-safe: atomic loads and updates,
/// `kill`, `signal`, `raise`, and `_exit`.
#[cfg(unix)]
extern "C" fn relay_signal(signal: libc::c_int) {
    LAST_SIGNAL.store(signal, Ordering::Relaxed);
    INTERRUPTS.fetch_add(1, Ordering::Release);
    let shared_with_group = signal == libc::SIGINT && holds_the_terminal();
    let spawning = SPAWNING.load(Ordering::Acquire) > 0
        && SPAWNING_INTERRUPTS.fetch_add(1, Ordering::Relaxed) < RELAYED_INTERRUPTS;
    let mut still_listening = spawning;
    let registered = visit_targets(|entry, target| {
        still_listening |= relay_to(entry, target, signal, shared_with_group);
    });
    // Nothing left to wait for, or every child has had its interrupt and
    // its `SIGTERM` and sat through both.
    if !(registered || spawning) || !still_listening {
        pnpm_fs::die_from_signal(signal);
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
    // SAFETY: `target` is a child pnpm spawned, or that child's process
    // group. `ESRCH` from one that exited concurrently is harmless.
    unsafe {
        libc::kill(target, relayed);
    }
    true
}

/// Whether pnpm's process group is the foreground group of its
/// controlling terminal, the group a `Ctrl+C` there interrupts as a whole.
///
/// The kernel does not say who sent a signal in a way every platform
/// agrees on (Linux marks the terminal's with `SI_KERNEL`, macOS marks
/// nothing), so the terminal is asked instead. A `kill` aimed at pnpm
/// alone while it holds the terminal is taken for the terminal's, and its
/// children get their signal on the next interrupt. Without a controlling
/// terminal, under a service manager or in a container, nothing but
/// `kill` can reach pnpm, and every signal is relayed.
///
/// `open`, `tcgetpgrp`, `getpgrp` and `close` are async-signal-safe.
#[cfg(unix)]
fn holds_the_terminal() -> bool {
    let Some(tty) = open_controlling_terminal() else {
        return false;
    };
    // SAFETY: `tty` is an open descriptor, closed here whatever
    // `tcgetpgrp` reports.
    unsafe {
        let foreground = libc::tcgetpgrp(tty);
        libc::close(tty);
        foreground == libc::getpgrp()
    }
}

/// Whether pnpm has a controlling terminal. Without one, only `kill` can
/// signal pnpm, and nothing signals its children but pnpm.
///
/// Async-signal-safe.
#[cfg(unix)]
pub(crate) fn has_controlling_terminal() -> bool {
    let Some(tty) = open_controlling_terminal() else {
        return false;
    };
    // SAFETY: `tty` is an open descriptor.
    unsafe {
        libc::close(tty);
    }
    true
}

/// Open `/dev/tty`, which succeeds only for a process with a controlling
/// terminal. `open` is async-signal-safe.
#[cfg(unix)]
fn open_controlling_terminal() -> Option<libc::c_int> {
    // SAFETY: the path is a NUL-terminated literal.
    let tty = unsafe {
        libc::open(c"/dev/tty".as_ptr(), libc::O_RDONLY | libc::O_NOCTTY | libc::O_CLOEXEC)
    };
    (tty >= 0).then_some(tty)
}

/// Close the handle a finished child left in an entry it no longer
/// occupies. Never called while the entry is published, which is the only
/// state a termination thread reads the handle in.
#[cfg(windows)]
fn close_retired_handle(handle: *mut std::ffi::c_void) {
    use windows_sys::Win32::Foundation::CloseHandle;

    if handle.is_null() {
        return;
    }
    // SAFETY: the handle came from `open_child_handle` and is closed once,
    // by the claim that retires it.
    unsafe {
        CloseHandle(handle);
    }
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
