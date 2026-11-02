//! Verification of the TUF repository that authenticates a registry's
//! `pnpr-log-policy.json`, following the client workflow of the
//! [TUF specification](https://theupdateframework.github.io/specification/latest/).
//!
//! The client is pinned to the first root version by its SHA-256, follows
//! root rotations one version at a time, each signed by the thresholds of
//! both the old and the new root, then verifies timestamp, snapshot, and
//! targets metadata for signatures, expiry, version consistency, and
//! rollback, and finally the target's length and hashes.
//!
//! It verifies Ed25519 keys only, as pnpr generates its repositories with
//! them, and reads only the top-level targets role. Signatures by keys of
//! other types count toward no threshold, and delegated targets are never
//! consulted, so a repository using them fails closed.

pub(crate) mod canonical;

pub use repository::*;

mod metadata;
mod repository;
mod role;

use derive_more::{Display, Error};

/// Why TUF metadata or a target was rejected.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum TufError {
    #[display("TUF metadata is not valid JSON: {_0}")]
    Json(serde_json::Error),
    #[display("TUF metadata has a non-integer number, which canonical JSON cannot encode")]
    NonIntegerNumber,
    #[display("expected {expected} metadata, found {actual}")]
    WrongType {
        #[error(not(source))]
        expected: &'static str,
        #[error(not(source))]
        actual: String,
    },
    #[display("unsupported TUF specification version {version}")]
    UnsupportedSpecVersion {
        #[error(not(source))]
        version: String,
    },
    #[display("the root does not match the pinned first root digest")]
    RootDigestMismatch,
    #[display("expected root version {expected}, found {actual}")]
    RootVersion { expected: u64, actual: u64 },
    #[display("the root does not define the {role} role")]
    MissingRole {
        #[error(not(source))]
        role: &'static str,
    },
    #[display("{role} metadata has {found} valid signatures, {required} required")]
    Threshold {
        #[error(not(source))]
        role: &'static str,
        found: usize,
        required: u64,
    },
    #[display("{role} metadata expired")]
    Expired {
        #[error(not(source))]
        role: &'static str,
    },
    #[display("{role} metadata has an invalid expiry")]
    InvalidExpiry {
        #[error(not(source))]
        role: &'static str,
    },
    #[display("{role} version {offered} is older than the trusted version {trusted}")]
    Rollback {
        #[error(not(source))]
        role: &'static str,
        trusted: u64,
        offered: u64,
    },
    #[display("{role} version {actual} does not match the version {expected} listed for it")]
    VersionMismatch {
        #[error(not(source))]
        role: &'static str,
        expected: u64,
        actual: u64,
    },
    #[display("{role} metadata does not list {file}")]
    MissingMeta {
        #[error(not(source))]
        role: &'static str,
        #[error(not(source))]
        file: &'static str,
    },
    #[display("{role} metadata must be loaded first")]
    NotLoaded {
        #[error(not(source))]
        role: &'static str,
    },
    #[display("{file} does not match the length or hashes listed for it")]
    FileMismatch {
        #[error(not(source))]
        file: String,
    },
    #[display("{file} is listed without a SHA-256 or SHA-512 hash")]
    NoSupportedHash {
        #[error(not(source))]
        file: String,
    },
    #[display("the trusted targets do not list {path}")]
    MissingTarget {
        #[error(not(source))]
        path: String,
    },
}

impl From<serde_json::Error> for TufError {
    fn from(error: serde_json::Error) -> Self {
        TufError::Json(error)
    }
}

#[cfg(test)]
mod tests;
