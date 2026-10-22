//! Safe abstractions over the terminal and console foreign calls pnpm
//! makes.
//!
//! One of the FFI abstraction crates the
//! [`unsafe` policy](../../../UNSAFE_POLICY.md) defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own. The console
//! handles never leave the crate, because a standard handle is borrowed
//! from the process and must not be closed by whoever asked for it.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

#[cfg(windows)]
pub use console::{enable_virtual_terminal_processing, stop_inheriting_standard_handles};
#[cfg(unix)]
pub use width::terminal_columns;

/// Which standard stream a query or a change is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Standard input.
    Input,
    /// Standard output.
    Output,
    /// Standard error.
    Error,
}

#[cfg(windows)]
mod console;
#[cfg(unix)]
mod width;
