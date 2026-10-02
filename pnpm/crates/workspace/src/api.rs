//! Capability traits and the [`Host`] provider for this crate.
//!
//! Each crate that needs to thread a side-effecting capability through
//! a generic seam declares its own capability traits and its own
//! `Host` provider; this is the one for `pnpm-workspace`.
//! Production callers turbofish [`Host`] explicitly; tests substitute
//! a per-test unit struct that implements only the bounds the
//! function under test declares.
//!
//! See the
//! [Dependency injection for tests](../../../CODE_STYLE_GUIDE.md#dependency-injection-for-tests)
//! section of the style guide for the full convention.

use std::ffi::OsString;

/// Capability: read a process environment variable as a raw
/// [`OsString`]. Mirrors [`std::env::var_os`].
pub trait EnvVarOs {
    /// Return the value of the named environment variable as an
    /// [`OsString`], or `None` when unset.
    fn var_os(name: &str) -> Option<OsString>;
}

/// Production provider for the capability traits in this crate.
pub struct Host;

impl EnvVarOs for Host {
    fn var_os(name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}
