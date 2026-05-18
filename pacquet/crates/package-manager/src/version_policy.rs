//! Parse and expand `<name>[@<version>[||<version>...]]` specs from
//! `pnpm-workspace.yaml`'s `allowBuilds` (and analogous policy keys).
//!
//! Ports the [`expandPackageVersionSpecs`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L17-L29)
//! half of upstream's `config/version-policy` crate, plus the
//! `parseVersionPolicyRule` and `parseExactVersionsUnion` helpers
//! it builds on
//! ([`config/version-policy/src/index.ts:56-91`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L56-L91)).
//!
//! What this supports:
//!
//! - Bare name → `foo`, `@scope/foo`.
//! - Exact version → `foo@1.0.0`, `@scope/foo@1.0.0`.
//! - Exact-version union → `foo@1.0.0 || 2.0.0`. Each version is
//!   parsed strictly (the JS-side uses `semver.valid`); whitespace
//!   around `||` and within versions is trimmed.
//!
//! What this does NOT support — matching upstream's
//! [`'should not allow patterns in allowBuilds'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/test/index.ts#L28-L34)
//! test:
//!
//! - Wildcards in the name (`is-*`, `@scope/*`). Upstream's
//!   `createAllowBuildFunction` uses `.has()` on the expanded set,
//!   so a `*` in the spec ends up as a literal `*` in the set and
//!   never matches a real package name. Pacquet preserves that
//!   semantics — `*` is allowed in the parsed name when there's no
//!   version part (so the literal string lands in the output set),
//!   but it does NOT match real packages. Combining `*` with a
//!   version is explicitly rejected as
//!   [`ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L73-L75)
//!   (the same diagnostic code upstream emits).
//!
//! Note: `createPackageVersionPolicy` (which DOES support
//! wildcards via `Matcher`) is a separate upstream function used
//! by `minimumReleaseAgeExclude` / `dlx` — pacquet doesn't have
//! those features yet, so only `expand_package_version_specs` is
//! ported.

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use std::collections::HashSet;

/// Error from [`expand_package_version_specs`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum VersionPolicyError {
    /// One of the versions in a `||` union didn't parse as a valid
    /// exact semver. Mirrors upstream's
    /// [`ERR_PNPM_INVALID_VERSION_UNION`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L67-L69).
    #[display("Invalid versions union. Found: \"{pattern}\". Use exact versions only.")]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_UNION))]
    InvalidVersionUnion {
        #[error(not(source))]
        pattern: String,
    },

    /// A `*` wildcard in the package name AND a version part were
    /// combined. Upstream rejects this because the matcher built on
    /// top of the expanded set is `.has()` — wildcards would be
    /// non-functional. Mirrors
    /// [`ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L73-L75).
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
/// [`expandPackageVersionSpecs`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L17-L29).
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

/// Parsed `<name>[@<version-union>]` rule. Either `exact_versions`
/// is empty (bare name) or it contains one or more concrete semver
/// strings. Mixing a `*` wildcard in the name with a version part
/// is rejected by the caller before this struct is returned.
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
