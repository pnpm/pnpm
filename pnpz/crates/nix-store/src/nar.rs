//! The Nix archive format: a deterministic serialization of one file tree,
//! holding regular files (with an executable flag), symlinks, and directories
//! whose entries are sorted by name. A store path's hash is the SHA-256 of it.
//!
//! A string is its length as a little-endian `u64`, its bytes, and zero
//! padding to a multiple of eight.

pub use hashing::HashingReader;
pub use pack::pack;
pub use unpack::{
    UnpackError,
    unpack,
};

mod hashing;
mod pack;
mod unpack;

const MAGIC: &str = "nix-archive-1";

/// The modification time Nix gives everything in the store, one second after
/// the epoch, so that no build output depends on when it was installed.
const STORE_MTIME_SECONDS: u64 = 1;

#[cfg(test)]
mod tests;
