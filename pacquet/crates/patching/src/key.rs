use node_semver::Version;

/// Result of parsing a `patchedDependencies` key.
///
/// Mirrors the subset of upstream's
/// [`parse`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/path/src/index.ts#L120-L168)
/// that [`groupPatchedDependencies`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/groupPatchedDependencies.ts#L19-L21)
/// uses. Patched-dependency keys never carry peer-graph or
/// patch-hash suffixes, so this parser only distinguishes:
///
/// - bare `name` → both fields are `None`,
/// - `name@<valid-semver>` → `version` is `Some`,
/// - `name@<anything-else>` → `non_semver_version` is `Some`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedKey<'a> {
    pub name: Option<&'a str>,
    pub version: Option<&'a str>,
    pub non_semver_version: Option<&'a str>,
}

/// Parse a `patchedDependencies` key.
///
/// Returns `ParsedKey::default()` (all `None`) when the input has no
/// `@` separator at index ≥ 1 — matching upstream where `dp.parse`
/// returns the empty object `{}`. Callers handle that case by treating
/// the entire key as a bare package name (wildcard match).
pub fn parse_key(input: &str) -> ParsedKey<'_> {
    // `indexOf('@', 1)` upstream — skip a leading `@` so scoped names
    // (`@scope/foo`) match on the *second* `@`.
    let bytes = input.as_bytes();
    let sep_index = bytes.iter().enumerate().skip(1).find_map(|(i, &b)| (b == b'@').then_some(i));

    let Some(sep) = sep_index else {
        return ParsedKey::default();
    };

    let name = &input[..sep];
    let version = &input[sep + 1..];
    if version.is_empty() {
        return ParsedKey::default();
    }

    if Version::parse(version).is_ok() {
        ParsedKey { name: Some(name), version: Some(version), non_semver_version: None }
    } else {
        ParsedKey { name: Some(name), version: None, non_semver_version: Some(version) }
    }
}

#[cfg(test)]
mod tests {
    use super::{ParsedKey, parse_key};
    use pretty_assertions::assert_eq;

    #[test]
    fn bare_name_returns_empty() {
        assert_eq!(parse_key("lodash"), ParsedKey::default());
    }

    #[test]
    fn bare_scoped_name_returns_empty() {
        // `@scope/foo` has `@` at index 0; `indexOf('@', 1)` upstream
        // skips it, and there is no second `@`. Falls into the
        // wildcard bucket.
        assert_eq!(parse_key("@scope/foo"), ParsedKey::default());
    }

    #[test]
    fn name_at_exact_version() {
        assert_eq!(
            parse_key("lodash@4.17.21"),
            ParsedKey { name: Some("lodash"), version: Some("4.17.21"), non_semver_version: None },
        );
    }

    #[test]
    fn scoped_name_at_exact_version() {
        assert_eq!(
            parse_key("@scope/foo@1.0.0"),
            ParsedKey {
                name: Some("@scope/foo"),
                version: Some("1.0.0"),
                non_semver_version: None,
            },
        );
    }

    #[test]
    fn name_at_range_becomes_non_semver() {
        assert_eq!(
            parse_key("lodash@^4.17.0"),
            ParsedKey { name: Some("lodash"), version: None, non_semver_version: Some("^4.17.0") },
        );
    }

    /// Per upstream `parse`: an empty version after the `@` separator
    /// drops back to the empty result. Matches the early `if (version)`
    /// guard at
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/deps/path/src/index.ts#L137>.
    #[test]
    fn empty_version_returns_empty() {
        assert_eq!(parse_key("lodash@"), ParsedKey::default());
    }

    /// `1.x.x` is a valid semver *range* but not a valid semver
    /// *version*. Upstream's `semver.valid()` returns null, so the
    /// parsed result lands in `nonSemverVersion`.
    #[test]
    fn version_with_x_is_range() {
        assert_eq!(
            parse_key("foo@1.x.x"),
            ParsedKey { name: Some("foo"), version: None, non_semver_version: Some("1.x.x") },
        );
    }
}
