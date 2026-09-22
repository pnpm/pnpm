//! What the fetchers and the parser in this crate refuse with.
//!
//! The codes are upstream's, which the install reporter classifies by:
//! see each variant for the stage it stands for.

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use std::{
    string::FromUtf8Error,
    sync::Arc,
};

/// Errors raised by [`crate::fetch_shasums_file`] and [`crate::fetch_shasums_file_raw`].
///
/// Mirrors upstream's `ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE` code, which the
/// install reporter parses as a network-stage failure.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum FetchShasumsFileError {
    #[display("Failed to fetch integrity file: {url} (status: {status})")]
    #[diagnostic(code(ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE))]
    StatusNotOk { url: String, status: u16 },

    #[display("Failed to fetch integrity file: {url}")]
    #[diagnostic(code(ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE))]
    Network {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("The integrity file {url} exceeds {limit} bytes")]
    #[diagnostic(code(ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE))]
    TooLarge {
        #[error(not(source))]
        url: String,
        limit: usize,
    },
}

/// Errors raised by [`crate::fetch_verified_node_shasums`] and
/// [`crate::fetch_verified_node_shasums_file`].
///
/// Mirrors pnpm's `ERR_PNPM_NODE_SHASUMS_FETCH_FAIL` and
/// `ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID` codes. These are specific to
/// Node.js runtime verification, where a repository-configurable
/// mirror cannot be trusted to supply both the binary and the hash
/// list unchecked.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum FetchVerifiedNodeShasumsError {
    #[display("Failed to fetch {what} ({url}) to verify the Node.js download (status: {status})")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_FETCH_FAIL))]
    StatusNotOk {
        #[error(not(source))]
        what: &'static str,
        #[error(not(source))]
        url: String,
        status: u16,
    },

    #[display("Failed to fetch {what} ({url}) to verify the Node.js download")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_FETCH_FAIL))]
    Network {
        #[error(not(source))]
        what: &'static str,
        #[error(not(source))]
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Could not read the Node.js SHASUMS signature: {error}")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    SignatureUnreadable {
        #[error(source)]
        error: Arc<pgp::errors::Error>,
    },

    #[display("The verified Node.js SHASUMS file at {url} is not valid UTF-8")]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    InvalidUtf8 {
        #[error(not(source))]
        url: String,
        #[error(source)]
        error: Arc<FromUtf8Error>,
    },

    #[display(
        "Embedded Node.js release key fingerprint mismatch: expected {expected}, got {actual}"
    )]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    EmbeddedKeyFingerprintMismatch {
        #[error(not(source))]
        expected: &'static str,
        #[error(not(source))]
        actual: String,
    },

    #[display(
        "The OpenPGP signature of {url} does not match any trusted Node.js release key. The downloaded Node.js runtime cannot be verified as a genuine release."
    )]
    #[diagnostic(code(ERR_PNPM_NODE_SHASUMS_SIGNATURE_INVALID))]
    SignatureInvalid {
        #[error(not(source))]
        url: String,
    },
}

/// Errors raised by [`crate::pick_file_checksum_from_shasums_file`].
///
/// Two upstream codes survive the port verbatim — they are the
/// per-file equivalents of `ERR_PNPM_FAILED_DOWNLOAD_SHASUM_FILE`'s download
/// failure and signal that the body the verifier already has does not
/// answer the question being asked.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PickFileChecksumError {
    #[display("SHA-256 hash not found in SHASUMS256.txt for: {file_name}")]
    #[diagnostic(code(ERR_PNPM_NODE_INTEGRITY_HASH_NOT_FOUND))]
    NotFound {
        #[error(not(source))]
        file_name: String,
    },

    #[display("Malformed SHA-256 for {file_name}: {sha256}")]
    #[diagnostic(code(ERR_PNPM_NODE_MALFORMED_INTEGRITY_HASH))]
    Malformed { file_name: String, sha256: String },
}
