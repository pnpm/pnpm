//! Names of the lockfile formats a resolve `done` frame carries, matching the
//! server's `lockfileFormat` field.

use super::PnprClientError;

/// A `pnpm-lock.yaml` document with `lockfileVersion: '9.0'`, as JSON. The
/// only format an npm resolve request accepts.
pub(crate) const PNPM_LOCK_V9: &str = "pnpm-lock/9.0";

/// A `Cargo.lock` document, as a TOML string.
pub(crate) const CARGO_LOCK: &str = "cargo-lock";

/// A `pylock.toml` document, as JSON.
pub(crate) const PYLOCK: &str = "pylock";

/// Refuse a `done` frame whose `lockfileFormat` is not the one this client
/// reads.
pub(crate) fn expect_lockfile_format(
    actual: Option<&str>,
    expected: &str,
) -> Result<(), PnprClientError> {
    if actual == Some(expected) {
        return Ok(());
    }
    Err(PnprClientError::Protocol(format!(
        "pnpr server returned lockfileFormat {actual:?}, expected {expected:?}",
    )))
}

/// Like [`expect_lockfile_format`], but also accepts a frame without one. A
/// Cargo or Python request has a single format, so a server that predates
/// `lockfileFormat` still answered in it.
pub(crate) fn expect_sole_lockfile_format(
    actual: Option<&str>,
    expected: &str,
) -> Result<(), PnprClientError> {
    expect_lockfile_format(actual.or(Some(expected)), expected)
}
