//! Capability traits and the project-wide [`Host`] provider.
//!
//! Mirrors the dependency-injection pattern documented in
//! [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339): one
//! trait per capability, one provider gathering every capability impl
//! used across the codebase, all methods static. Production callers
//! turbofish the real provider explicitly
//! (e.g. `Config::current::<Host>(...)`); tests substitute a
//! per-test unit struct that implements only the bounds the function
//! actually declares, with any per-test scenario data stored in a
//! `static` inside the test fn.
//!
//! Today the provider only exposes [`EnvVar`]. As more side-effecting
//! capabilities are introduced (filesystem, disk inspection, time,
//! …) their `impl … for Host` blocks land here too, so callers
//! never juggle multiple providers. Trait names keep their domain
//! prefix (`Fs*`, `GetDisk*`, `Env*`, …) so a reader can identify
//! which domain a generic bound belongs to without chasing
//! definitions.

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
