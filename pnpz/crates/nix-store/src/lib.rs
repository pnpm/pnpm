//! The Nix formats pnpz reads and writes: store paths, NAR archives, and the
//! `.narinfo` documents of a binary cache with their signatures.
//!
//! Nothing here performs IO beyond the reader or directory a caller hands in.

pub mod base32;
#[cfg(unix)]
pub mod nar;

pub use nar_hash::{
    NarHash,
    ParseNarHashError,
};
pub use narinfo::{
    Compression,
    NarInfo,
    ParseNarInfoError,
};
pub use signature::{
    NIXOS_CACHE_KEY,
    ParseTrustedKeyError,
    SignatureError,
    TrustedKey,
};
pub use store_path::{
    ParseStorePathError,
    STORE_DIR,
    StorePath,
};

mod nar_hash;
mod narinfo;
mod signature;
mod store_path;
