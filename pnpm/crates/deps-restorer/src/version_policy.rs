//! Re-export of the `pnpm_config::version_policy` parsers for
//! `build_modules.rs`: `expand_package_version_specs` for `allowBuilds`,
//! and `create_package_version_policy` for `sideEffectsCacheExclude`.

pub use pnpm_config::version_policy::{
    PackageVersionPolicy, PolicyMatch, VersionPolicyError, create_package_version_policy,
    expand_package_version_specs,
};
