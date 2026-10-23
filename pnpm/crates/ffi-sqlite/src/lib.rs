//! Safe abstractions over the `SQLite` build-configuration queries pnpm
//! makes.
//!
//! One of the FFI abstraction crates the
//! `unsafe` policy in `UNSAFE_POLICY.md` defines: the `unsafe` here
//! is the crate's purpose, and no caller needs any of its own.
//!
//! `rusqlite` is a safe wrapper and covers everything pnpm does with a
//! database. It leaves one gap: the check that the library was built
//! thread-safe, which it performs on every connection it opens except on
//! `wasm32`, where it compiles the check out.

#![allow(unsafe_code, reason = "exempt by UNSAFE_POLICY.md, enforced by pnpm-unsafe-audit")]

/// Whether the `SQLite` the process is linked against was built thread-safe.
///
/// A database opened on a single-threaded build hands out a connection that
/// is not safe to share, and the WASI build shares one across its workers.
/// Ask before opening, so the refusal names the build rather than arriving
/// as a crash later.
#[must_use]
pub fn is_threadsafe() -> bool {
    // SAFETY: `sqlite3_threadsafe` reports a value fixed when the library
    // was compiled. It takes no arguments, touches no memory, needs no
    // initialized library, and cannot fail.
    unsafe { rusqlite::ffi::sqlite3_threadsafe() != 0 }
}

#[cfg(test)]
mod tests;
