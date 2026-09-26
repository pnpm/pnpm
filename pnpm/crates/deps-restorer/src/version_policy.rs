//! Re-export of `expand_package_version_specs` for `build_modules.rs`'s
//! `allowBuilds` consumer.
//!
//! The implementation lives in
//! [`pnpm_config::version_policy`] so the matcher-based
//! [`create_package_version_policy`](pnpm_config::version_policy::create_package_version_policy)
//! sibling — needed by `minimumReleaseAgeExclude` /
//! `trustPolicyExclude` — can share the same parser.

pub use pnpm_config::version_policy::{VersionPolicyError, expand_package_version_specs};
