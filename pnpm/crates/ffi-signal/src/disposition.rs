//! Installing a signal handler, and passing the signal on to what the
//! handler replaced.

use crate::Interrupt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A handler for one interrupt.
///
/// It runs in a signal handler, so what it calls must be
/// async-signal-safe: no allocating, no locking, no unwinding. Reading and
/// writing atomics is, and so is anything
/// [`signal(7)`](https://man7.org/linux/man-pages/man7/signal.7.html)
/// lists.
///
/// The second argument is what the signal did before pnpm's handler was
/// installed. A handler that decides pnpm should go on running does
/// nothing with it; one that decides pnpm should stop passes the signal on
/// with [`Replaced::pass_on`].
pub type Handler = fn(Interrupt, Replaced);

/// Install `handler` for each of `interrupts`.
///
/// An interrupt the process was started ignoring is left ignored, so pnpm
/// under `nohup` or in a background job keeps that disposition and its
/// children inherit it across `exec`.
///
/// A second call for the same interrupt replaces the handler the first
/// installed, and [`Replaced`] keeps naming the disposition from before
/// pnpm installed anything, which is the one that decides whether the
/// process dies. The replaced pnpm handler is not reachable afterwards.
pub fn install(interrupts: &[Interrupt], handler: Handler) {
    for interrupt in interrupts {
        install_one(*interrupt, handler);
    }
}

/// Whether `interrupt` already carries a handler, rather than the default
/// or ignored disposition.
///
/// A caller that installs behind a process-global guard asks this to find
/// out whether something else got there first. An interrupt whose
/// disposition cannot be read reads as unhandled, which is what
/// [`install`] treats it as.
#[must_use]
pub fn carries_handler(interrupt: Interrupt) -> bool {
    current_disposition(slot(interrupt).number)
        .is_some_and(|disposition| {
            disposition.sa_sigaction != libc::SIG_DFL && disposition.sa_sigaction != libc::SIG_IGN
        })
}

/// What an interrupt did before pnpm installed a handler for it.
///
/// Handed to a [`Handler`], which is the only place it can be used: the
/// disposition it names is only correct for the signal being delivered.
pub struct Replaced {
    slot: &'static Slot,
    siginfo: *mut libc::siginfo_t,
    context: *mut libc::c_void,
}

impl Replaced {
    /// Whether the replaced disposition ends the process.
    ///
    /// A handler that only has cleanup to do checks this: where it is
    /// `true`, nothing runs after the signal, so the cleanup has to happen
    /// now.
    #[must_use]
    pub fn ends_the_process(&self) -> bool {
        self.slot.was_default.load(Ordering::Acquire)
    }

    /// Pass the signal on to the handler pnpm's replaced, or do nothing
    /// where the replaced disposition was the default or ignore.
    ///
    /// Async-signal-safe.
    pub fn pass_on(self) {
        let previous = self.slot.previous.load(Ordering::Acquire);
        if previous == libc::SIG_DFL || previous == libc::SIG_IGN {
            return;
        }
        let number = self.slot.number;
        if self.slot.takes_siginfo.load(Ordering::Acquire) {
            // SAFETY: `previous` is the `sa_sigaction` read back at install
            // time from a disposition whose `sa_flags` carried
            // `SA_SIGINFO`, so it is the address of a function of exactly
            // this signature, and a handler stays installed for the life of
            // the process. The two pointers are the ones this delivery was
            // handed, which is what that signature is called with.
            let handler: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                unsafe { std::mem::transmute(previous) };
            handler(number, self.siginfo, self.context);
        } else {
            // SAFETY: `previous` is the `sa_sigaction` read back at install
            // time from a disposition whose `sa_flags` did not carry
            // `SA_SIGINFO`, so the union held `sa_handler`: the address of a
            // function of exactly this signature. A handler stays installed
            // for the life of the process.
            let handler: extern "C" fn(libc::c_int) = unsafe { std::mem::transmute(previous) };
            handler(number);
        }
    }
}

/// What is known about one interrupt, written once at install time and read
/// from the signal handler.
struct Slot {
    number: libc::c_int,
    /// The `sa_sigaction` of the disposition from before pnpm installed
    /// anything. `SIG_DFL` until an install records one.
    previous: AtomicUsize,
    /// Whether [`Self::previous`] was installed with `SA_SIGINFO`, and so
    /// takes three arguments rather than one. The union `sigaction` stores
    /// the two forms in does not say which it holds, and calling one
    /// through the other's signature is undefined, so the flag is read
    /// alongside the pointer and kept with it.
    takes_siginfo: AtomicBool,
    was_default: AtomicBool,
    /// The pnpm handler currently installed, or null before the first
    /// install.
    handler: AtomicUsize,
}

impl Slot {
    const fn new(number: libc::c_int) -> Self {
        Self {
            number,
            previous: AtomicUsize::new(libc::SIG_DFL),
            takes_siginfo: AtomicBool::new(false),
            was_default: AtomicBool::new(true),
            handler: AtomicUsize::new(0),
        }
    }
}

/// One slot per [`Interrupt`], in a fixed table because a signal handler
/// cannot allocate or lock to find its entry.
static SLOTS: [Slot; 3] =
    [Slot::new(libc::SIGINT), Slot::new(libc::SIGTERM), Slot::new(libc::SIGHUP)];

fn slot(interrupt: Interrupt) -> &'static Slot {
    &SLOTS[match interrupt {
        Interrupt::Interrupt => 0,
        Interrupt::Terminate => 1,
        Interrupt::Hangup => 2,
    }]
}

fn install_one(interrupt: Interrupt, handler: Handler) {
    let slot = slot(interrupt);
    let Some(previous) = current_disposition(slot.number) else { return };
    // An ignored interrupt is left ignored, and nothing is recorded for it,
    // so a later install sees the same untouched disposition.
    if previous.sa_sigaction == libc::SIG_IGN {
        return;
    }
    // Only the first install records what it replaced: a second one would
    // otherwise record this crate's own trampoline, and passing the signal
    // on would re-enter it forever.
    if slot.handler.load(Ordering::Acquire) == 0 {
        slot.previous.store(previous.sa_sigaction, Ordering::Release);
        slot.takes_siginfo.store(previous.sa_flags & libc::SA_SIGINFO != 0, Ordering::Release);
        slot.was_default.store(previous.sa_sigaction == libc::SIG_DFL, Ordering::Release);
    }
    slot.handler.store(handler as usize, Ordering::Release);
    install_trampoline(slot.number);
}

/// The disposition `number` has right now, or `None` when it cannot be
/// read, which leaves it untouched.
fn current_disposition(number: libc::c_int) -> Option<libc::sigaction> {
    // SAFETY: `sigaction` holds nothing but integers and function
    // addresses, so an all-zero value is a valid one to read into, and the
    // struct has reserved fields a literal cannot name.
    let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: the output pointer is to a fully initialized stack local that
    // outlives the call, and a null `act` asks only for the current
    // disposition without changing it.
    let read = unsafe { libc::sigaction(number, std::ptr::null(), &raw mut previous) };
    (read == 0).then_some(previous)
}

fn install_trampoline(number: libc::c_int) {
    // SAFETY: as in `current_disposition`, an all-zero `sigaction` is a
    // valid one to build on, and the struct's reserved fields cannot be
    // named by a literal.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    // `SA_SIGINFO` so the trampoline receives the delivery's `siginfo_t`
    // and context, which is what lets `pass_on` forward them to a replaced
    // handler that was itself installed with `SA_SIGINFO`.
    //
    // `SA_RESTART` keeps the syscalls running underneath from failing with
    // `EINTR` when the handler fires.
    action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
    action.sa_sigaction = trampoline as *const () as usize;
    // SAFETY: the pointer is to a fully initialized stack local that
    // outlives the call. `sigemptyset` writes the mask through it, and
    // `sigaction` reads the whole struct and retains no pointer. A null
    // `oldact` discards the disposition being replaced, which
    // `current_disposition` already read.
    unsafe {
        libc::sigemptyset(&raw mut action.sa_mask);
        libc::sigaction(number, &raw const action, std::ptr::null_mut());
    }
}

/// The handler every installed interrupt actually gets, which finds the
/// caller's and calls it.
///
/// Three arguments because the trampoline is installed with `SA_SIGINFO`:
/// the pointers are the delivery's own, and are passed through to a
/// replaced handler of the same shape rather than invented.
extern "C" fn trampoline(
    number: libc::c_int,
    siginfo: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    let Some(interrupt) = interrupt_of(number) else { return };
    let slot = slot(interrupt);
    let handler = slot.handler.load(Ordering::Acquire);
    if handler == 0 {
        return;
    }
    // SAFETY: `handler` is the address of the `Handler` an install stored,
    // which is an ordinary `fn` item of exactly this signature and lives
    // for the whole program.
    let handler: Handler = unsafe { std::mem::transmute(handler) };
    handler(interrupt, Replaced { slot, siginfo, context });
}

fn interrupt_of(number: libc::c_int) -> Option<Interrupt> {
    match number {
        libc::SIGINT => Some(Interrupt::Interrupt),
        libc::SIGTERM => Some(Interrupt::Terminate),
        libc::SIGHUP => Some(Interrupt::Hangup),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
