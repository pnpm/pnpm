//! Evaluates whether a package can be installed on the current host.

pub use check_engine::{
    Engine, InvalidNodeVersionError, UnsupportedEngineError, WantedEngine, check_engine,
};
pub use check_platform::{
    Platform, UnsupportedPlatformError, WantedPlatform, WantedPlatformRef, check_platform,
    platform_is_supported,
};
pub use infer_platform_from_package_name::{infer_platform_from_package_name, inferred_platform};
pub use package_is_installable::{
    InstallabilityError, InstallabilityOptions, InstallabilityVerdict,
    PackageInstallabilityManifest, SkipReason, check_package, package_is_installable,
    platform_is_supported_with_inference,
};
pub use supported_architectures::{
    ArchitectureAxes, SupportedArchitectures,
    platform::{
        Architecture, Libc, LibcFamily, NamedPlatform, Os, SupportedPlatform, UnknownPlatformError,
    },
};

mod check_engine;
mod check_platform;
mod infer_platform_from_package_name;
mod package_is_installable;
mod supported_architectures;

#[cfg(test)]
mod tests;
