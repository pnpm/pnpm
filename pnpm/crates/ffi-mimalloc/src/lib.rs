//! Safe abstractions over the mimalloc options pnpm reads and writes.
//!
//! One of the FFI abstraction crates the
//! `unsafe` policy in `UNSAFE_POLICY.md` defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own.
//!
//! `libmimalloc-sys` declares the option calls. What it does not declare
//! is the purge-delay option itself, whose neighbours on either side it
//! binds, so the index lives here with the calls that take it.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

use std::ffi::{c_int, c_long};

/// `mi_option_purge_delay` in `mimalloc.h`.
const PURGE_DELAY: c_int = 15;

/// How long mimalloc waits, in milliseconds, before returning freed
/// memory to the operating system.
#[must_use]
pub fn purge_delay() -> c_long {
    // SAFETY: `libmimalloc-sys` links the library that defines this
    // function, and the index is a valid option in mimalloc v2 and v3.
    unsafe { libmimalloc_sys::mi_option_get(PURGE_DELAY) }
}

/// Set the purge delay that applies when `MIMALLOC_PURGE_DELAY` does not.
///
/// Call it before the process starts another thread: the write is
/// unsynchronized, so a concurrent reader would race it.
pub fn set_default_purge_delay(milliseconds: c_long) {
    // SAFETY: `libmimalloc-sys` links the library that defines this
    // function, and the index is a valid option in mimalloc v2 and v3.
    // The call writes the option without synchronization, which the
    // documented precondition on this function hands to the caller.
    unsafe { libmimalloc_sys::mi_option_set_default(PURGE_DELAY, milliseconds) };
}
