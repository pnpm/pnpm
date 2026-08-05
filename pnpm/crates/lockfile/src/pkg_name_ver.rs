use crate::{ParsePkgNameSuffixError, PkgNameSuffix};
use node_semver::{SemverError, Version};

/// Syntax: `{name}@{version}`
///
/// Examples: `ts-node@10.9.1`, `@types/node@18.7.19`, `typescript@5.1.6`
pub type PkgNameVer = PkgNameSuffix<Version>;

/// Error when parsing [`PkgNameVer`] from a string.
pub type ParsePkgNameVerError = ParsePkgNameSuffixError<SemverError>;

#[cfg(test)]
mod tests;
