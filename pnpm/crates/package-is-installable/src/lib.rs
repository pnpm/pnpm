//! Evaluates whether a package can be installed on the current host.
//!
//! Exported functions:
//! - [`check_engine()`] — evaluates `engines.node` / `engines.pnpm` against
//!   the current runtime.
//! - [`check_platform()`] — evaluates a package's `os` / `cpu` / `libc`
//!   triple against the host (or a caller-supplied
//!   [`SupportedArchitectures`] override).
//! - [`platform_is_supported()`] — the allocation-light boolean form of
//!   the same platform check.
//! - [`package_is_installable()`] — composes the two and produces a
//!   tri-state verdict: compatible, skip-as-optional, or
//!   proceed-with-warning. Caller handles emitting `pnpm:install-check`
//!   and `pnpm:skipped-optional-dependency` events.

mod check_engine;
mod check_platform;
mod infer_platform_from_package_name;
mod package_is_installable;

#[cfg(test)]
mod tests;

pub use check_engine::{
    Engine, InvalidNodeVersionError, UnsupportedEngineError, WantedEngine, check_engine,
};
pub use check_platform::{
    Platform, SupportedArchitectures, UnsupportedPlatformError, WantedPlatform, WantedPlatformRef,
    check_platform, platform_is_supported,
};
pub use infer_platform_from_package_name::{infer_platform_from_package_name, inferred_platform};
pub use package_is_installable::{
    InstallabilityError, InstallabilityOptions, InstallabilityVerdict,
    PackageInstallabilityManifest, SkipReason, check_package, package_is_installable,
};
