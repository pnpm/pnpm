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

use std::ffi::{
    c_int,
    c_long,
};

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

#[cfg(feature = "purge-freed-memory-immediately")]
ctor::declarative::ctor! {
    /// Purge freed memory without a delay, unless `MIMALLOC_PURGE_DELAY` says
    /// otherwise.
    ///
    /// mimalloc purges freed memory only while a thread is allocating, and by
    /// default only after a delay, so memory freed by the last requests of a
    /// burst stays resident for as long as a server is idle.
    ///
    /// The option write is unsynchronized, so it runs as a constructor: before
    /// `main`, no other thread exists to read the option.
    // SAFETY: the constructor runs before `main` and touches nothing of
    // `std` that needs initializing. Its one foreign call is argued below.
    #[ctor(unsafe)]
    fn purge_freed_memory_immediately() {
        // SAFETY: `libmimalloc-sys` links the library that defines this
        // function, and the index is a valid option in mimalloc v2 and
        // v3. The write is unsynchronized, and before `main` no other
        // thread exists to read the option.
        unsafe { libmimalloc_sys::mi_option_set_default(PURGE_DELAY, 0) };
    }
}
