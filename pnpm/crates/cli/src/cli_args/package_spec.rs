use pnpm_fs::lexical_normalize;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use std::path::Path;

/// Rewrite dot-relative `file:`, `link:`, and bare path selectors against
/// `base_dir`. Bare package names, home-relative (`~/`), and absolute
/// selectors pass through unchanged so the local resolver's `~` expansion and
/// registry fallbacks still apply.
pub(crate) fn resolve_local_param(param: &str, base_dir: &Path) -> String {
    for prefix in ["file:", "link:"] {
        if let Some(rest) = param.strip_prefix(prefix) {
            if rest.starts_with('.') {
                return format!("{prefix}{}", lexical_normalize(&base_dir.join(rest)).display());
            }
            return param.to_string();
        }
    }
    if param.starts_with('.') {
        return lexical_normalize(&base_dir.join(param)).display().to_string();
    }
    param.to_string()
}

/// Anchor dot-relative local path specs (both bare and `<alias>@<path>`)
/// against `base_dir`.
pub(crate) fn resolve_local_package_spec(spec: &str, base_dir: &Path) -> String {
    let parsed = parse_wanted_dependency(spec);
    match (parsed.alias, parsed.bare_specifier) {
        (Some(alias), Some(bare_specifier)) => {
            let resolved = resolve_local_param(&bare_specifier, base_dir);
            if resolved == bare_specifier {
                spec.to_string()
            } else {
                format!("{alias}@{resolved}")
            }
        }
        _ => resolve_local_param(spec, base_dir),
    }
}

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
