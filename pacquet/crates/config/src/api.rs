//! Capability traits and the [`Host`] provider for this crate.
//!
//! Each crate that needs to thread a side-effecting capability through
//! a generic seam declares its own capability traits and its own
//! `Host` provider; this is the one for `pacquet-config`. Production
//! callers turbofish the real provider explicitly
//! (e.g. `Config::current::<Host>(...)`); tests substitute a per-test
//! unit struct that implements only the bounds the function actually
//! declares, with any per-test scenario data stored in a `static`
//! inside the test fn.
//!
//! Today this provider only exposes [`EnvVar`]. As more side-effecting
//! capabilities are introduced into `pacquet-config` (filesystem reads
//! for `.npmrc`, network probes for auth, …) their `impl … for Host`
//! blocks land here too. Trait names keep their domain prefix (`Fs*`,
//! `GetDisk*`, `Env*`, …) so a reader can identify which domain a
//! generic bound belongs to without chasing definitions. See the
//! [Dependency injection for tests](../../../CODE_STYLE_GUIDE.md#dependency-injection-for-tests)
//! section of the style guide for the full convention.

/// Capability: read a process environment variable.
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

/// Project-wide capability provider. Production code threads
/// `Host` through generic call sites with an explicit turbofish:
///
/// ```ignore
/// let config = Config::current::<Host>(env::current_dir, home::home_dir, Default::default);
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
