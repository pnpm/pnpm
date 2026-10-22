//! Safe abstractions over the Apple `Security.framework` calls pnpm makes.
//!
//! One of the FFI abstraction crates the
//! [`unsafe` policy](../../../UNSAFE_POLICY.md) defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own.
//!
//! `security-framework` already wraps most of this framework safely. What
//! it does not offer is a constructor that reports the framework declining
//! to build an object, which is the one shape a restricted `trustd` takes
//! that pnpm has to recognise.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

#[cfg(target_vendor = "apple")]
pub use ssl_policy::server_ssl_policy;

#[cfg(target_vendor = "apple")]
mod ssl_policy;
