//! Port of `@pnpm/config.package-is-installable` from upstream pnpm.
//!
//! Mirrors the entry point and the two checker helpers
//! ([`check_engine()`], [`check_platform()`]) at
//! <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/>.
//!
//! Three exported functions:
//! - [`check_engine()`] — evaluates `engines.node` / `engines.pnpm` against
//!   the current runtime.
//! - [`check_platform()`] — evaluates a package's `os` / `cpu` / `libc`
//!   triple against the host (or a caller-supplied
//!   [`SupportedArchitectures`] override).
//! - [`package_is_installable()`] — composes the two and produces a
//!   tri-state verdict matching upstream's `boolean | null` return:
//!   compatible, skip-as-optional, or proceed-with-warning. Caller
//!   handles emitting `pnpm:install-check` and
//!   `pnpm:skipped-optional-dependency` events.

mod check_engine;
mod check_platform;
mod package_is_installable;

#[cfg(test)]
mod tests;

pub use check_engine::{
    Engine, InvalidNodeVersionError, UnsupportedEngineError, WantedEngine, check_engine,
};
pub use check_platform::{
    Platform, SupportedArchitectures, UnsupportedPlatformError, WantedPlatform, WantedPlatformRef,
    check_platform,
};
pub use package_is_installable::{
    InstallabilityError, InstallabilityOptions, InstallabilityVerdict,
    PackageInstallabilityManifest, SkipReason, check_package, package_is_installable,
};
