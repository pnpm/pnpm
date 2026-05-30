//! Capability traits and the [`Host`] provider for this crate.
//!
//! Each crate that needs to thread a side-effecting capability through
//! a generic seam declares its own capability traits and its own
//! `Host` provider; this is the one for `pacquet-workspace`.
//! Production callers turbofish [`Host`] explicitly; tests substitute
//! a per-test unit struct that implements only the bounds the
//! function under test declares.
//!
//! See the
//! [Dependency injection for tests](../../../CODE_STYLE_GUIDE.md#dependency-injection-for-tests)
//! section of the style guide for the full convention.

use std::ffi::OsString;

/// Capability: read a process environment variable as a raw
/// [`OsString`]. Mirrors [`std::env::var_os`]. Used by
/// [`crate::find_workspace_dir_from_env`] to resolve
/// `NPM_CONFIG_WORKSPACE_DIR` without mutating the real process
/// environment in tests.
pub trait EnvVarOs {
    /// Return the value of the named environment variable as an
    /// [`OsString`], or `None` when unset.
    fn var_os(name: &str) -> Option<OsString>;
}

/// Production provider for the capability traits in this crate.
/// Production code threads `Host` through generic call sites with an
/// explicit turbofish.
pub struct Host;

impl EnvVarOs for Host {
    fn var_os(name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}
