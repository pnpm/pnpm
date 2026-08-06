//! Compatibility shims that make `node-semver` (Rust) range checks agree
//! with JavaScript's `semver.validRange`, which the TypeScript pnpm CLI
//! resolves manifests with.
//!
//! The dialects diverge on the *empty comparator set*. A range is a union
//! of comparator sets separated by `||`, and JS reads an empty set as
//! "any version" — so `""`, whitespace, `"||"`, and `"^1.0.0 || "` all
//! normalize to `"*"`. The Rust parser rejects the first three outright
//! and silently drops the empty half of the last one. Registries are full
//! of published manifests that rely on the JS reading (`js-xlsx`,
//! `codepage`, and `ssf` all declare dependencies as `""`), so range
//! checks on manifest input must go through here rather than calling
//! [`Range::parse`] directly.
//!
//! JS has two readings of a union whose *non-empty* members do not parse,
//! and pnpm uses both: loose mode drops the members it cannot parse,
//! while strict mode rejects the whole range. The TypeScript CLI reaches
//! loose mode through `version-selector-type` (whose default export
//! passes `loose: true`) and strict mode through its own bare
//! `semver.validRange(..)` calls, so the two are not interchangeable —
//! see each function for which call sites it belongs to.

use node_semver::Range;

/// The range every any-version spelling normalizes to.
pub const ANY_VERSION_RANGE: &str = "*";

/// Whether `range` resolves to the unbounded range, matching JS
/// `semver.validRange(range, /* loose */ true) === "*"`.
///
/// An empty comparator set imposes no bound and loose mode discards the
/// sets it cannot parse, so a single empty member makes the whole union
/// unbounded however its siblings are spelled.
///
/// This is the reading a version-selector classifier wants, because that
/// is the mode `version-selector-type` runs in.
#[must_use]
pub fn is_any_version_range(range: &str) -> bool {
    range.split("||").any(|comparator_set| comparator_set.trim().is_empty())
}

/// The Rust equivalent of JS `semver.validRange(range) != null` — the
/// strict reading, under which one unparsable comparator set invalidates
/// the whole union rather than being dropped from it.
///
/// Use this instead of `Range::parse(range).is_ok()` wherever the
/// TypeScript CLI calls `semver.validRange` without `loose`: the
/// specifier-shape disambiguations that decide whether a body is a
/// version range or a package name. Reading those loosely would hand
/// `npm:bar@^5 || ` to the range branch and lose the `bar` alias.
#[must_use]
pub fn is_valid_semver_range(range: &str) -> bool {
    range
        .split("||")
        .map(str::trim)
        .all(|comparator_set| comparator_set.is_empty() || Range::parse(comparator_set).is_ok())
}

#[cfg(test)]
mod tests;
