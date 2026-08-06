//! Compatibility shims that make `node-semver` (Rust) range checks agree
//! with JavaScript's `semver.validRange`, which every pnpm version up to
//! v11 resolved manifests with.
//!
//! The dialects diverge on the *empty comparator set*. JS treats it as
//! "any version", so `""`, whitespace, `"||"`, and `"^1.0.0 || "` all
//! normalize to `"*"`. The Rust parser rejects the first three outright
//! and silently drops the empty half of the last one. Registries are full
//! of published manifests that rely on the JS reading (`js-xlsx`,
//! `codepage`, and `ssf` all declare dependencies as `""`), so range
//! checks on manifest input must go through here rather than calling
//! [`Range::parse`] directly.

use node_semver::Range;

/// The range every any-version spelling normalizes to.
pub const ANY_VERSION_RANGE: &str = "*";

/// Whether `range` is one of the spellings JS `semver.validRange`
/// normalizes to `"*"`.
///
/// A range is a union of comparator sets separated by `||`, and an empty
/// comparator set imposes no bound — so a single empty member makes the
/// whole union unbounded, however many other members it has.
#[must_use]
pub fn is_any_version_range(range: &str) -> bool {
    range.split("||").any(|comparator_set| comparator_set.trim().is_empty())
}

/// The Rust equivalent of JS `semver.validRange(range) != null`.
///
/// Use this instead of `Range::parse(range).is_ok()` whenever the input
/// comes from a package manifest or a user-written specifier, so an
/// omitted range is read as "any version" instead of falling through to
/// the dist-tag branch of a specifier classifier.
#[must_use]
pub fn is_valid_semver_range(range: &str) -> bool {
    is_any_version_range(range) || Range::parse(range).is_ok()
}

#[cfg(test)]
mod tests;
