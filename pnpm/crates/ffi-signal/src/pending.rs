//! Counting the interrupts the kernel holds queued for pnpm.

use crate::{Interrupt, disposition::signal_number};

/// How many of `interrupts` the kernel holds queued for pnpm with no
/// handler run for them yet.
///
/// An interrupt is queued rather than delivered only while it is blocked,
/// so the query blocks all of `interrupts` for its own length and then puts
/// the mask back, which lets this thread take the delivery of any that was
/// already waiting.
///
/// A query whose mask cannot be changed, or whose queue cannot be read,
/// counts nothing: the caller is then where it was before asking, which a
/// guess would not leave it.
#[must_use]
pub fn pending_count(interrupts: &[Interrupt]) -> usize {
    let asked = set_of(interrupts);
    let Some(previous) = block(&asked) else { return 0 };
    let queued = queued_signals();
    set_mask(&previous);
    let Some(queued) = queued else { return 0 };
    interrupts
        .iter()
        .filter(|interrupt| holds(&queued, signal_number(**interrupt)))
        .count()
}

/// An empty signal set with each of `interrupts` added.
fn set_of(interrupts: &[Interrupt]) -> libc::sigset_t {
    // SAFETY: `sigset_t` is a set of bits, so an all-zero value is a valid
    // one to build on, and the struct has reserved fields a literal cannot
    // name.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: the pointer is to a fully initialized stack local that
    // outlives every call, which write the set through it and retain
    // nothing.
    unsafe {
        libc::sigemptyset(&raw mut set);
        for interrupt in interrupts {
            libc::sigaddset(&raw mut set, signal_number(*interrupt));
        }
    }
    set
}

/// Block every signal in `set` for this thread, reporting the mask that was
/// in place, or `None` when the mask could not be changed.
fn block(set: &libc::sigset_t) -> Option<libc::sigset_t> {
    // SAFETY: as in `set_of`, an all-zero `sigset_t` is a valid one to read
    // into.
    let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to fully initialized stack locals that
    // outlive the call, which reads `set` and writes `previous` through
    // them and retains neither.
    let blocked =
        unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &raw const *set, &raw mut previous) };
    (blocked == 0).then_some(previous)
}

/// Put `mask` back as this thread's signal mask.
fn set_mask(mask: &libc::sigset_t) {
    // SAFETY: the input pointer is to a fully initialized stack local that
    // outlives the call, and a null `oldset` discards the mask being
    // replaced.
    unsafe {
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const *mask, std::ptr::null_mut());
    }
}

/// The signals queued for this thread, or `None` when the queue could not
/// be read.
fn queued_signals() -> Option<libc::sigset_t> {
    // SAFETY: as in `set_of`, an all-zero `sigset_t` is a valid one to read
    // into.
    let mut queued: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: the pointer is to a fully initialized stack local that
    // outlives the call, which writes the set through it and retains
    // nothing.
    let read = unsafe { libc::sigpending(&raw mut queued) };
    (read == 0).then_some(queued)
}

/// Whether `set` holds `signal`. A set that cannot answer reads as not
/// holding it.
fn holds(set: &libc::sigset_t, signal: libc::c_int) -> bool {
    // SAFETY: the pointer is to a fully initialized stack local that
    // outlives the call, which reads the set through it and writes nothing.
    unsafe { libc::sigismember(&raw const *set, signal) == 1 }
}

#[cfg(test)]
mod tests;
