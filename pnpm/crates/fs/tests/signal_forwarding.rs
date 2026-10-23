//! The forwarding of a delivery to the handler pnpm replaced.
//!
//! # This file must hold exactly one test
//!
//! A pristine process is this test's condition, not a convenience. A file
//! under `tests/` is its own binary, so the test below gets a process where
//! the `Once` behind [`pnpm_fs::install_temp_file_cleanup`] is unspent. That
//! is what lets the harness install its handler *underneath* pnpm's, so
//! pnpm's handler records it as the disposition it replaced and has
//! something to forward to. The altered `SIGTERM` then dies with the
//! process.
//!
//! Anything else added here that tracks a temp file or installs the cleanup
//! spends that `Once` first. pnpm's handler is then already in place, the
//! harness's handler goes on top of it instead of underneath, and the raise
//! reaches it directly: every assertion below still passes while no
//! forwarding has run at all. A second test here does not break this one
//! loudly, it hollows it out.
//!
//! So the test asserts the disposition it starts from rather than trusting
//! it, and that assertion is the thing to read if it ever fails.

#![cfg(unix)]

use pnpm_ffi_signal::{Interrupt, carries_handler};
use pnpm_testing_utils::signal_delivery::forwarded_delivery;

/// Asserts on the `siginfo_t` the replaced handler received rather than only
/// on its being reached, because a wrong signature leaves those arguments
/// holding whatever the registers did.
#[test]
fn a_replaced_siginfo_handler_is_passed_the_signal_details() {
    assert!(
        !carries_handler(Interrupt::Terminate),
        "SIGTERM already carries a handler, so the `Once` behind \
         install_temp_file_cleanup is spent and this test would assert \
         nothing: see the comment at the top of this file",
    );

    let delivery = forwarded_delivery(libc::SIGTERM, pnpm_fs::install_temp_file_cleanup);

    assert_eq!(
        delivery.signal,
        libc::SIGTERM,
        "the signal was not passed on to the handler pnpm replaced",
    );
    assert!(
        delivery.details_matched,
        "the replaced handler was called without the delivery's `siginfo_t`",
    );
}

// Do not add a test below this line. The comment at the top of the file says
// why: a second test in this binary spends the `Once` and leaves the test
// above passing without forwarding anything.
