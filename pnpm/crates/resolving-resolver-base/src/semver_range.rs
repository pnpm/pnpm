//! Compatibility shims that make `node-semver` (Rust) range checks agree
//! with JavaScript's `semver.validRange`, which the TypeScript pnpm CLI
//! resolves manifests with.
//!
//! The dialects diverge on the *empty comparator set*: a range is a union
//! of comparator sets separated by `||`, and JS reads an empty set as
//! "any version" where the Rust parser rejects it. Registries are full of
//! published manifests that rely on the JS reading — `js-xlsx`,
//! `codepage`, and `ssf` all declare dependencies as `""` — so range
//! checks on manifest input must go through here rather than calling
//! [`Range::parse`] directly.
//!
//! JS has a loose and a strict reading of a union whose non-empty members
//! do not parse, and pnpm uses both. The two functions below model one
//! each and are not interchangeable.

use node_semver::Range;

/// The range every any-version spelling normalizes to.
pub const ANY_VERSION_RANGE: &str = "*";

/// Whether `range` resolves to the unbounded range, matching JS
/// `semver.validRange(range, /* loose */ true) === "*"`.
///
/// Loose mode discards the comparator sets it cannot parse, so a single
/// empty member makes the whole union unbounded however its siblings are
/// spelled. This is the mode `version-selector-type` runs in, and so the
/// reading a version-selector classifier wants.
#[must_use]
pub fn is_any_version_range(range: &str) -> bool {
    range.split("||").any(|comparator_set| comparator_set.trim().is_empty())
}

/// The Rust equivalent of JS `semver.validRange(range) != null` — the
/// strict reading, under which one unparsable comparator set invalidates
/// the whole union rather than being dropped from it.
///
/// Belongs wherever the TypeScript CLI calls `semver.validRange` without
/// `loose`: the specifier-shape disambiguations that decide whether a
/// body is a version range or a package name.
#[must_use]
pub fn is_valid_semver_range(range: &str) -> bool {
    range
        .split("||")
        .map(str::trim)
        .all(|comparator_set| comparator_set.is_empty() || Range::parse(comparator_set).is_ok())
}

#[cfg(test)]
mod tests;
