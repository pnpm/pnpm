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
#[must_use]
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
mod tests;
