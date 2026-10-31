use super::{current_disposition, interrupt_of};
use crate::Interrupt;

/// Pins that the three interrupts map to the signal numbers the slot table
/// is built from, so a delivery finds its own slot rather than another's.
#[test]
fn every_interrupt_maps_to_its_signal_and_back() {
    for (interrupt, number) in [
        (Interrupt::Interrupt, libc::SIGINT),
        (Interrupt::Terminate, libc::SIGTERM),
        (Interrupt::Hangup, libc::SIGHUP),
    ] {
        assert_eq!(interrupt_of(number), Some(interrupt));
        assert_eq!(super::slot(interrupt).number, number);
    }
}

/// A signal pnpm does not install for is not one of its interrupts, so the
/// trampoline returns rather than indexing the table with it.
#[test]
fn an_unhandled_signal_maps_to_no_interrupt() {
    assert_eq!(interrupt_of(libc::SIGUSR1), None);
}

/// Pins that the flag deciding which signature a replaced handler is called
/// through is read from the disposition rather than assumed.
///
/// `sigaction` stores the one-argument and three-argument handlers in a
/// union, so nothing but `SA_SIGINFO` distinguishes them, and calling one
/// through the other's signature is undefined.
///
/// What this pins is the read: that the address and the flag come back as
/// installed, for a handler installed each way. That the flag then decides
/// which signature a replaced handler is called through is pinned by
/// `a_replaced_siginfo_handler_is_passed_the_signal_details` in
/// `pnpm-fs`, which aborts if the two are confused.
#[test]
fn the_siginfo_flag_is_read_back_from_the_disposition() {
    extern "C" fn plain(_: libc::c_int) {}
    extern "C" fn detailed(_: libc::c_int, _: *mut libc::siginfo_t, _: *mut libc::c_void) {}

    // `SIGUSR2` because pnpm installs no handler for it, so this cannot
    // disturb the dispositions the crate records.
    let flags_for = |handler: usize, flags: libc::c_int| {
        // SAFETY: an all-zero `sigaction` is a valid one to build on, and
        // the pointers are to fully initialized stack locals that outlive
        // the calls.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_flags = flags;
            action.sa_sigaction = handler;
            libc::sigemptyset(&raw mut action.sa_mask);
            assert_eq!(libc::sigaction(libc::SIGUSR2, &raw const action, std::ptr::null_mut()), 0);
        }
        let read = current_disposition(libc::SIGUSR2).expect("the disposition is readable");
        (read.sa_sigaction, read.sa_flags & libc::SA_SIGINFO != 0)
    };

    let (address, takes_siginfo) = flags_for(plain as *const () as usize, 0);
    assert_eq!(address, plain as *const () as usize);
    assert!(!takes_siginfo, "a handler installed without SA_SIGINFO read as taking siginfo");

    let (address, takes_siginfo) = flags_for(detailed as *const () as usize, libc::SA_SIGINFO);
    assert_eq!(address, detailed as *const () as usize);
    assert!(takes_siginfo, "a handler installed with SA_SIGINFO read as not taking siginfo");
}

/// An ignored interrupt is left ignored, so pnpm under `nohup` keeps the
/// disposition its children inherit.
#[test]
fn an_ignored_signal_keeps_its_disposition() {
    // SAFETY: `signal` takes both arguments by value and writes no memory.
    // `SIGUSR2` is one pnpm installs no handler for.
    unsafe {
        libc::signal(libc::SIGUSR2, libc::SIG_IGN);
    }
    let read = current_disposition(libc::SIGUSR2).expect("the disposition is readable");
    assert_eq!(read.sa_sigaction, libc::SIG_IGN);
}
