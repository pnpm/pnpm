use super::interrupt_count;
use pnpm_testing_utils::pending_signal::while_pending;

/// A delivery the kernel has queued but run no handler for counts as
/// arrived, so a script's exit read before the handler runs is still read
/// as one the user ended.
#[test]
fn a_pending_signal_counts_as_an_interrupt() {
    let before = interrupt_count();

    let while_queued = while_pending(libc::SIGHUP, interrupt_count);

    assert_eq!(while_queued, before + 1);
    assert_eq!(interrupt_count(), before);
}
