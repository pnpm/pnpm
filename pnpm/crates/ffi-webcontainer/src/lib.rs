//! Safe abstractions over the `WebContainer` supervisor's imports.
//!
//! One of the FFI abstraction crates the
//! [`unsafe` policy](../../../UNSAFE_POLICY.md) defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own. The raw
//! declarations are in [`pnpm-webcontainer-sys`](../webcontainer-sys).
//!
//! The supervisor retains an operation's result until it is read or
//! cancelled. Guest memory is borrowed only during an import call, never
//! while an operation is pending.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

#[cfg(target_family = "wasm")]
pub mod fs;

#[cfg(target_family = "wasm")]
pub use request::{HostError, Request, close_resource, request};

#[cfg(target_family = "wasm")]
mod request;
