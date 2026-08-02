use node_semver::Version;

/// Version-slot prefixes with a fixed meaning in dep paths, resolution ids,
/// or dependency specifiers (`foo@file:...`, `node@runtime:...`, `npm:`, ...).
/// A named-registry alias may not shadow any of them: the registry-qualified
/// dep path form `<name>@<alias>:<version>` would otherwise be ambiguous.
pub const RESERVED_VERSION_PREFIXES: &[&str] = &[
    "bitbucket",
    "catalog",
    "custom",
    "file",
    "git",
    "github",
    "gitlab",
    "http",
    "https",
    "jsr",
    "link",
    "npm",
    "runtime",
    "ssh",
    "workspace",
];

/// Whether `name` is one of [`RESERVED_VERSION_PREFIXES`].
#[must_use]
pub fn is_reserved_version_prefix(name: &str) -> bool {
    RESERVED_VERSION_PREFIXES.contains(&name)
}

/// Whether `name` is syntactically usable as a named-registry alias in a
/// registry-qualified dep path. Does not check the reserved list —
/// see [`is_reserved_version_prefix`] for that.
#[must_use]
pub fn is_well_formed_registry_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-'))
}

/// Parse the version slot of a registry-qualified dep path
/// (`<registryName>:<version>`, e.g. `work:1.0.0`) — the form used for
/// packages resolved from a named registry since lockfile format 12.0.
/// Returns `None` for every other version form (plain semver, `file:`,
/// `runtime:`, git/tarball URLs, ...).
#[must_use]
pub fn parse_registry_qualified_version(version: &str) -> Option<(&str, Version)> {
    let colon = version.find(':')?;
    let registry_name = &version[..colon];
    if registry_name.is_empty()
        || is_reserved_version_prefix(registry_name)
        || !is_well_formed_registry_name(registry_name)
    {
        return None;
    }
    let qualified_version = version[colon + 1..].parse::<Version>().ok()?;
    Some((registry_name, qualified_version))
}

#[cfg(test)]
mod tests;
