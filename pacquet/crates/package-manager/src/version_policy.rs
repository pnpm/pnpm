//! Back-compat re-export of the `expand_package_version_specs`
//! function that originally lived here.
//!
//! The implementation moved to
//! [`pacquet_config::version_policy`] so the matcher-based
//! [`create_package_version_policy`](pacquet_config::version_policy::create_package_version_policy)
//! sibling — needed by `minimumReleaseAgeExclude` /
//! `trustPolicyExclude` — can share the same parser. This module
//! keeps the old import path working for `build_modules.rs`'s
//! `allowBuilds` consumer.

pub use pacquet_config::version_policy::{VersionPolicyError, expand_package_version_specs};
