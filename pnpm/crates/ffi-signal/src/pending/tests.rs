use super::pending_count;
use crate::Interrupt;
use pnpm_testing_utils::pending_signal::while_pending;

/// Every interrupt pnpm handles, which is what a caller that installed a
/// handler for all of them asks about.
const EVERY: [Interrupt; 3] = [Interrupt::Interrupt, Interrupt::Terminate, Interrupt::Hangup];

/// A queued interrupt is counted while it is queued, and no longer once the
/// delivery has been taken off the queue.
#[test]
fn a_queued_interrupt_is_counted_until_it_is_taken() {
    assert_eq!(pending_count(&EVERY), 0);
    assert_eq!(while_pending(libc::SIGHUP, || pending_count(&EVERY)), 1);
    assert_eq!(pending_count(&EVERY), 0);
}

/// Only the interrupts the caller asks about are counted, so a caller that
/// handles some of them is not told about the rest.
#[test]
fn an_interrupt_the_caller_does_not_ask_about_is_not_counted() {
    const WITHOUT_HANGUP: [Interrupt; 2] = [Interrupt::Interrupt, Interrupt::Terminate];

    assert_eq!(while_pending(libc::SIGHUP, || pending_count(&[Interrupt::Hangup])), 1);
    assert_eq!(while_pending(libc::SIGHUP, || pending_count(&WITHOUT_HANGUP)), 0);
}

/// Asking about nothing counts nothing, however much is queued.
#[test]
fn asking_about_no_interrupt_counts_nothing() {
    assert_eq!(while_pending(libc::SIGHUP, || pending_count(&[])), 0);
}
