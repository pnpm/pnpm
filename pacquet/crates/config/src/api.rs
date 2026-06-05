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
//! Trait names keep their domain prefix (`Env*`, `Get*`, ...) so a
//! reader can identify which side effect a generic bound belongs to
//! without chasing definitions. See the
//! [Dependency injection for tests](../../../CODE_STYLE_GUIDE.md#dependency-injection-for-tests)
//! section of the style guide for the full convention.

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

/// Capability: read a process environment variable as a UTF-8 string.
///
/// Defined in the `pacquet-env-replace` crate and re-exported here so
/// this crate's callers keep importing it from `pacquet_config` alongside
/// the other capability traits. [`Host`] implements it for production code.
pub use pacquet_env_replace::EnvVar;

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
/// genuinely needs the cwd — the `SmartDefault` for
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

/// Capability: probe whether a file dropped into `from_dir` could be
/// hardlinked into a subdirectory of `to_dir`.
///
/// Port of pnpm's
/// [`canLinkToSubdir`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L80-L92),
/// abstracted as a single yes/no question so the production impl owns
/// all filesystem effects (creating the source file, the destination
/// temp dir, the link attempt, and cleanup) and tests can answer
/// without touching disk. Lets pacquet's port of
/// [`storePathRelativeToHome`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L45-L78)
/// drive its branches deterministically.
///
/// "Linkable" means
/// [`std::fs::hard_link`] returns `Ok(())`; everything else (EXDEV,
/// EACCES, EPERM, ENOSPC, missing parent dir, ...) is treated as "not
/// linkable" and the caller falls through to the next branch.
/// Mirrors pnpm's
/// [`canLink`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L3-L18)
/// catch-all: any failure means "this volume is not a candidate," not
/// "the install should abort."
pub trait LinkProbe {
    /// Return `true` when a hardlink can be created from a file in
    /// `from_dir` into a subdirectory of `to_dir`, `false` otherwise.
    /// Implementations must clean up any temp file or directory they
    /// create — no filesystem effects survive the call.
    fn can_link_between_dirs(from_dir: &Path, to_dir: &Path) -> bool;
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

impl LinkProbe for Host {
    fn can_link_between_dirs(from_dir: &Path, to_dir: &Path) -> bool {
        crate::store_path::host_can_link_between_dirs(from_dir, to_dir)
    }
}
