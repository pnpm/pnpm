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
/// The `NPM_CONFIG_WORKSPACE_DIR` lookup in `findWorkspaceDir` goes
/// through this trait so tests can drive the "set", "unset", and
/// "empty" branches without touching process state.
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
/// drive-letter derivation, and [`crate::Config::current`] anchors a
/// relative `npmrcAuthFile` value at the cwd (matching where the file
/// is actually read from, and pnpm's `path.resolve`). Code that needs
/// a "starting path" — like [`crate::Config::current`] — otherwise
/// takes a direct path parameter, because production passes a
/// caller-supplied path (the canonicalized `--dir`) rather than the
/// host's cwd.
pub trait GetCurrentDir {
    /// Return the process's current working directory, or an error
    /// if it can't be determined. Mirrors [`std::env::current_dir`].
    fn current_dir() -> io::Result<PathBuf>;
}

/// Capability: probe whether a file dropped into `from_dir` could be
/// hardlinked into a subdirectory of `to_dir`.
///
/// Abstracted as a single yes/no question so the production impl owns
/// all filesystem effects (creating the source file, the destination
/// temp dir, the link attempt, and cleanup) and tests can answer
/// without touching disk. Lets `store_path_relative_to_home` drive its
/// branches deterministically.
///
/// "Linkable" means
/// [`std::fs::hard_link`] returns `Ok(())`; everything else (EXDEV,
/// EACCES, EPERM, ENOSPC, missing parent dir, ...) is treated as "not
/// linkable" and the caller falls through to the next branch.
/// Any failure means "this volume is not a candidate," not "the
/// install should abort."
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

    fn vars() -> Vec<(String, String)> {
        // `std::env::vars()` panics on non-UTF-8 entries; iterate the
        // OsString form and skip those, matching `var`'s `.ok()` behavior.
        std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect()
    }
}

impl EnvVarOs for Host {
    fn var_os(name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

impl GetHomeDir for Host {
    fn home_dir() -> Option<PathBuf> {
        if let Ok(sudo_user) = std::env::var("SUDO_USER") {
            if sudo_user != "root" {
                #[cfg(all(unix, not(target_os = "cygwin")))]
                {
                    use std::ffi::CString;
                    if let Ok(c_user) = CString::new(sudo_user) {
                        // SAFETY: calling getpwnam is safe and returns a pointer to a static struct or null.
                        unsafe {
                            let pw = libc::getpwnam(c_user.as_ptr());
                            if !pw.is_null() {
                                let c_str = std::ffi::CStr::from_ptr((*pw).pw_dir);
                                if let Ok(s) = c_str.to_str() {
                                    return Some(PathBuf::from(s));
                                }
                            }
                        }
                    }
                }
            }
        }
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
