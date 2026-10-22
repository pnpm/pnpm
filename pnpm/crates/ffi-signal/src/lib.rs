//! Safe abstractions over the signal and console-event foreign calls pnpm
//! makes.
//!
//! One of the FFI abstraction crates the
//! [`unsafe` policy](../../../UNSAFE_POLICY.md) defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own.
//!
//! A handler installed through this crate is an ordinary Rust `fn`, and
//! what it may do inside is the constraint the platform imposes rather than
//! one this crate adds: no allocating, no locking, no unwinding. See
//! `install` on Unix and `on_console_event` on Windows.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

#[cfg(windows)]
pub use console::{Handled, on_console_event};
#[cfg(unix)]
pub use death::die_from;
#[cfg(unix)]
pub use disposition::{Handler, Replaced, install};
#[cfg(unix)]
pub use relay::{
    Target, group_has_members, has_controlling_terminal, holds_the_terminal, process_group_of,
    reap, signal_target, target_exists,
};

/// An interrupt pnpm handles, under the name it has on both platforms.
///
/// Unix delivers these as signals and Windows as console control events.
/// The two platforms agree on what the first two mean, so a handler is
/// written once; `Hangup` has no console event, and never fires there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// `Ctrl+C` at the terminal: `SIGINT`, or `CTRL_C_EVENT`.
    Interrupt,
    /// A service manager or `kill` asking the process to stop: `SIGTERM`,
    /// or `CTRL_BREAK_EVENT`, which `Ctrl+Break` also raises.
    Terminate,
    /// The terminal going away: `SIGHUP`. Windows has no console event for
    /// it.
    Hangup,
}

#[cfg(windows)]
mod console;
#[cfg(unix)]
mod death;
#[cfg(unix)]
mod disposition;
#[cfg(unix)]
mod relay;
