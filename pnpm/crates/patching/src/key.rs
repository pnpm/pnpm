use node_semver::Version;

/// Result of parsing a `patchedDependencies` key.
///
/// Patched-dependency keys never carry peer-graph or patch-hash
/// suffixes, so only the name and version slots are parsed out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedKey<'a> {
    pub name: Option<&'a str>,
    pub version: Option<&'a str>,
    pub non_semver_version: Option<&'a str>,
}

/// Parse a `patchedDependencies` key.
///
/// Returns `ParsedKey::default()` (all `None`) when the input has no
/// `@` separator at index ≥ 1. Callers handle that case by treating
/// the entire key as a bare package name (wildcard match).
#[must_use]
pub fn parse_key(input: &str) -> ParsedKey<'_> {
    // Skip a leading `@` so scoped names (`@scope/foo`) match on the
    // *second* `@`.
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
