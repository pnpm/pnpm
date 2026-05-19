//! Capability traits and the [`Host`] provider for this crate.
//!
//! Each crate that needs to thread a side-effecting capability through
//! a generic seam declares its own capability traits and its own
//! `Host` provider; this is the one for `pacquet-config`. Production
//! callers turbofish the real provider explicitly
//! (e.g. `Config::default().current::<Host>(...)`); tests substitute a per-test
//! unit struct that implements only the bounds the function actually
//! declares, with any per-test scenario data stored in a `static`
//! inside the test fn.
//!
//! Trait names keep their domain prefix (`Env*`, `Get*`, …) so a
//! reader can identify which side effect a generic bound belongs to
//! without chasing definitions. See the
//! [Dependency injection for tests](../../../CODE_STYLE_GUIDE.md#dependency-injection-for-tests)
//! section of the style guide for the full convention.

use std::{ffi::OsString, io, path::PathBuf};

/// Capability: read a process environment variable as a UTF-8 string.
///
/// `pnpm` resolves `${VAR}` placeholders inside `.npmrc` against the
/// process environment in
/// [`loadNpmrcFiles.ts`](https://github.com/pnpm/pnpm/blob/601317e7a3/config/reader/src/loadNpmrcFiles.ts#L156-L162);
/// pacquet routes that lookup through this trait so unit tests can
/// drive every branch (set, unset, empty) with local fakes instead
/// of mutating the real process environment.
pub trait EnvVar {
    /// Return the value of the named environment variable, or `None`
    /// when it is unset. Implementations should treat invalid UTF-8
    /// as `None` to match `std::env::var`'s behaviour, which is what
    /// pnpm itself observes via Node's `process.env`.
    fn var(name: &str) -> Option<String>;
}

/// Capability: read a process environment variable as a raw
/// [`OsString`]. Used for env vars whose value is a filesystem path
/// — invalid UTF-8 is preserved verbatim so the path can be passed
/// to `std::fs` without round-tripping through `String`.
///
/// Pacquet's `NPM_CONFIG_WORKSPACE_DIR` lookup (mirroring upstream's
/// `findWorkspaceDir`) goes through this trait so tests can drive
/// the "set", "unset", and "empty" branches without touching
/// process state.
pub trait EnvVarOs {
    /// Return the value of the named environment variable as an
    /// [`OsString`], or `None` when unset. Mirrors
    /// [`std::env::var_os`].
    fn var_os(name: &str) -> Option<OsString>;
}

/// Capability: locate the user's home directory.
///
/// Mirrors the [`home::home_dir`] crate function. Threaded through a
/// trait so tests don't have to consult the host's actual home
/// directory.
pub trait GetHomeDir {
    /// Return the user's home directory, or `None` when it can't be
    /// determined. Mirrors [`home::home_dir`].
    fn home_dir() -> Option<PathBuf>;
}

/// Capability: read the process's current working directory.
///
/// Mirrors [`std::env::current_dir`]. Only used by code that
/// genuinely needs the cwd — the SmartDefault for
/// [`crate::Config::store_dir`] consults it on Windows for the
/// drive-letter derivation. Code that needs a "starting path" — like
/// [`crate::Config::current`] — takes a direct path parameter
/// instead, because production passes a caller-supplied path (the
/// canonicalized `--dir`) rather than the host's cwd.
pub trait GetCurrentDir {
    /// Return the process's current working directory, or an error
    /// if it can't be determined. Mirrors [`std::env::current_dir`].
    fn current_dir() -> io::Result<PathBuf>;
}

/// Production provider for the capability traits in this crate.
/// Production code threads `Host` through generic call sites with an
/// explicit turbofish:
///
/// ```ignore
/// let config = Config::default().current::<Host>(&dir);
/// ```
///
/// Tests substitute their own zero-sized struct that implements only
/// the trait bounds the function under test declares.
pub struct Host;

impl EnvVar for Host {
    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl EnvVarOs for Host {
    fn var_os(name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

impl GetHomeDir for Host {
    fn home_dir() -> Option<PathBuf> {
        home::home_dir()
    }
}

impl GetCurrentDir for Host {
    fn current_dir() -> io::Result<PathBuf> {
        std::env::current_dir()
    }
}
