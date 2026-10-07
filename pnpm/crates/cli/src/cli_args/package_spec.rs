use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;

/// A parsed package specification from a CLI argument.
///
/// Contains a validated npm package name and an optional version, tag, or semver range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    pub name: String,
    pub version: Option<String>,
}

impl PackageSpec {
    /// Parse a package specifier string like `foo`, `foo@1.0.0`, `@scope/foo`, or `@scope/foo@latest`.
    ///
    /// Trims surrounding whitespace. Returns `None` if the input does not begin with
    /// a valid package name.
    pub fn parse(spec: &str) -> Option<Self> {
        let trimmed = spec.trim();
        let parsed = parse_wanted_dependency(trimmed);
        let name = parsed.alias?;
        let version = parsed.bare_specifier.filter(|version| !version.is_empty());
        Some(Self { name, version })
    }
}

#[cfg(test)]
mod tests;
