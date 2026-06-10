//! Parse `<name>[@<version>[||<version>...]]` specs from
//! `pnpm-workspace.yaml`'s `allowBuilds`, `minimumReleaseAgeExclude`,
//! `trustPolicyExclude`, and similar policy keys.
//!
//! Ports the relevant halves of upstream's
//! [`config/version-policy`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts):
//!
//! - [`expand_package_version_specs`] expands every spec into one or
//!   more literal `name` / `name@version` strings, matching upstream's
//!   `expandPackageVersionSpecs`. Used by `allowBuilds`.
//! - [`create_package_version_policy`] returns a matcher-based policy
//!   that evaluates a `pkg_name` against a set of rules, mirroring
//!   upstream's `createPackageVersionPolicy`. Used by
//!   `minimumReleaseAgeExclude` and `trustPolicyExclude` — wildcards
//!   in the name (`is-*`, `@scope/*`) match real package names via the
//!   shared [`crate::matcher`].
//!
//! What this module supports:
//!
//! - Bare name → `foo`, `@scope/foo`.
//! - Exact version → `foo@1.0.0`, `@scope/foo@1.0.0`.
//! - Exact-version union → `foo@1.0.0 || 2.0.0`. Each version is
//!   parsed strictly (upstream uses `semver.valid`); whitespace
//!   around `||` and within versions is trimmed.
//! - Wildcards in the name **without** a version part —
//!   [`expand_package_version_specs`] keeps them verbatim (matches
//!   upstream's `.has()` semantics where the literal lands in the
//!   `Set`), and [`create_package_version_policy`] runs them through
//!   [`crate::matcher`] so they match real package names.
//!
//! Combining a `*` wildcard in the name with a version part is
//! explicitly rejected as
//! [`VersionPolicyError::NamePatternInVersionUnion`].

use crate::matcher::{Matcher, create_matcher};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use std::collections::HashSet;

/// Error from [`expand_package_version_specs`] or
/// [`create_package_version_policy`]. Mirrors the two upstream codes
/// at <https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L67-L75>.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum VersionPolicyError {
    /// One of the versions in a `||` union didn't parse as a valid
    /// exact semver. Mirrors upstream's
    /// [`ERR_PNPM_INVALID_VERSION_UNION`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L67-L69).
    #[display("Invalid versions union. Found: \"{pattern}\". Use exact versions only.")]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_UNION))]
    InvalidVersionUnion {
        #[error(not(source))]
        pattern: String,
    },

    /// A `*` wildcard in the package name AND a version part were
    /// combined. Upstream rejects this because the resulting matcher
    /// would have inconsistent semantics with the rest of the rule
    /// set. Mirrors
    /// [`ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L73-L75).
    #[display("Name patterns are not allowed with version unions. Found: \"{pattern}\"")]
    #[diagnostic(code(ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION))]
    NamePatternInVersionUnion {
        #[error(not(source))]
        pattern: String,
    },
}

/// Expand each spec into one or more `name` / `name@version` literal
/// strings.
///
/// Output shape:
///
/// - Bare `foo` → `{"foo"}`.
/// - `foo@1.0.0` → `{"foo@1.0.0"}`.
/// - `foo@1.0.0 || 2.0.0` → `{"foo@1.0.0", "foo@2.0.0"}`.
/// - `@scope/foo@1.0.0` → `{"@scope/foo@1.0.0"}`.
///
/// Ports upstream's
/// [`expandPackageVersionSpecs`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L59-L72).
/// Callers feed the result into a `HashSet::contains` check, so a
/// pattern like `is-*` lands in the set as the literal string
/// `"is-*"` and never matches a real package name (matches upstream
/// behavior exactly — see `should not allow patterns in allowBuilds`
/// in `building/policy/test/index.ts`).
pub fn expand_package_version_specs<Iter, Spec>(
    specs: Iter,
) -> Result<HashSet<String>, VersionPolicyError>
where
    Iter: IntoIterator<Item = Spec>,
    Spec: AsRef<str>,
{
    let mut out: HashSet<String> = HashSet::new();
    for spec in specs {
        let parsed = parse_version_policy_rule(spec.as_ref())?;
        if parsed.exact_versions.is_empty() {
            out.insert(parsed.package_name.to_string());
        } else {
            for version in parsed.exact_versions {
                out.insert(format!("{}@{}", parsed.package_name, version));
            }
        }
    }
    Ok(out)
}

/// Decision a [`PackageVersionPolicy`] reaches for a given package name.
/// Mirrors upstream's `boolean | string[]` return shape at
/// [`evaluateVersionPolicy`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L74-L85).
///
/// - [`PolicyMatch::No`] — no rule matched the name (upstream's `false`).
/// - [`PolicyMatch::AnyVersion`] — a bare-name rule matched (upstream's
///   `true`). Every version of the package is covered.
/// - [`PolicyMatch::ExactVersions`] — a name+version rule matched. Only
///   the listed versions are covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyMatch {
    No,
    AnyVersion,
    ExactVersions(Vec<String>),
}

/// Matcher-based version policy built from a list of
/// `<name-pattern>[@<version>||<version>...]` rules. Rules are walked
/// in order; the first whose name matcher accepts the input package
/// name wins. Mirrors upstream's [`createPackageVersionPolicy`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L6-L13).
///
/// Used by `minimumReleaseAgeExclude` and `trustPolicyExclude`, both
/// of which need wildcard name patterns (`is-*`, `@scope/*`) AND
/// exact version unions (`lodash@4.17.21 || 4.17.22`) — different from
/// `allowBuilds`, which lands as a literal set via
/// [`expand_package_version_specs`].
#[derive(Clone)]
pub struct PackageVersionPolicy {
    rules: Vec<VersionPolicyRule>,
}

impl std::fmt::Debug for PackageVersionPolicy {
    // `Matcher` doesn't expose its compiled pattern set, so the
    // most useful thing the debug rendering can show is the rule
    // count and each rule's exact-versions list.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageVersionPolicy")
            .field("rules", &self.rules.iter().map(|rule| &rule.exact_versions).collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone)]
struct VersionPolicyRule {
    name_matcher: Matcher,
    exact_versions: Vec<String>,
}

impl PackageVersionPolicy {
    /// Evaluate the policy against a package name. Returns the
    /// matching rule's payload (`AnyVersion` for a bare-name rule,
    /// `ExactVersions` for `name@versions...`), or `PolicyMatch::No`
    /// when no rule matched.
    #[must_use]
    pub fn matches(&self, pkg_name: &str) -> PolicyMatch {
        for rule in &self.rules {
            if !rule.name_matcher.matches(pkg_name) {
                continue;
            }
            return if rule.exact_versions.is_empty() {
                PolicyMatch::AnyVersion
            } else {
                PolicyMatch::ExactVersions(rule.exact_versions.clone())
            };
        }
        PolicyMatch::No
    }
}

/// Compile a list of `<name-pattern>[@<version>||<version>...]` rules
/// into a [`PackageVersionPolicy`]. Port of upstream's
/// [`createPackageVersionPolicy`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/version-policy/src/index.ts#L6-L13).
///
/// Errors mirror upstream:
///
/// - A `||` union that contains a non-semver value →
///   [`VersionPolicyError::InvalidVersionUnion`].
/// - A `*` wildcard in the name combined with a version part →
///   [`VersionPolicyError::NamePatternInVersionUnion`].
pub fn create_package_version_policy<Iter, Spec>(
    patterns: Iter,
) -> Result<PackageVersionPolicy, VersionPolicyError>
where
    Iter: IntoIterator<Item = Spec>,
    Spec: AsRef<str>,
{
    let mut rules: Vec<VersionPolicyRule> = Vec::new();
    for pattern in patterns {
        let parsed = parse_version_policy_rule(pattern.as_ref())?;
        // [`create_matcher`] takes a slice of patterns; we pass a single
        // entry per rule so the rule's own matcher returns true on a
        // name hit and falls through otherwise.
        let name_matcher = create_matcher(&[parsed.package_name.to_string()]);
        rules.push(VersionPolicyRule { name_matcher, exact_versions: parsed.exact_versions });
    }
    Ok(PackageVersionPolicy { rules })
}

/// Parsed `<name>[@<version-union>]` rule. Either `exact_versions`
/// is empty (bare name) or it contains one or more concrete semver
/// strings. Mixing a `*` wildcard in the name with a version part
/// is rejected by [`parse_version_policy_rule`] before this struct
/// is returned.
struct ParsedRule<'a> {
    package_name: &'a str,
    exact_versions: Vec<String>,
}

fn parse_version_policy_rule(pattern: &str) -> Result<ParsedRule<'_>, VersionPolicyError> {
    // Scoped name (`@scope/foo`) starts with `@`, so the version
    // separator is the *second* `@`. Otherwise the first.
    let at_index = if pattern.starts_with('@') {
        pattern.char_indices().skip(1).find_map(|(i, c)| (c == '@').then_some(i))
    } else {
        pattern.find('@')
    };

    let Some(at) = at_index else {
        return Ok(ParsedRule { package_name: pattern, exact_versions: Vec::new() });
    };

    let package_name = &pattern[..at];
    let versions_part = &pattern[at + 1..];

    let exact_versions = parse_exact_versions_union(versions_part)
        .ok_or_else(|| VersionPolicyError::InvalidVersionUnion { pattern: pattern.to_string() })?;

    if package_name.contains('*') {
        return Err(VersionPolicyError::NamePatternInVersionUnion { pattern: pattern.to_string() });
    }

    Ok(ParsedRule { package_name, exact_versions })
}

/// Parse `v1 || v2 || …` into a list of strict semver versions.
/// Returns `None` if any component fails to parse — the caller
/// surfaces that as `ERR_PNPM_INVALID_VERSION_UNION`. Whitespace
/// around `||` and around each version is trimmed before parsing
/// (matches Node-semver's `valid()` which trims internally).
fn parse_exact_versions_union(versions_str: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in versions_str.split("||") {
        let trimmed = raw.trim();
        let version = Version::parse(trimmed).ok()?;
        out.push(version.to_string());
    }
    Some(out)
}

#[cfg(test)]
mod tests;
