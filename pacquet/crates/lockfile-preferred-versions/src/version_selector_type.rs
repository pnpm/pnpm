//! Port of the `version-selector-type` npm package
//! (<https://github.com/pnpm/version-selector-type/blob/3f9669ce4d/index.js>).
//!
//! Classifies a manifest specifier as a concrete `Version`, a semver
//! `Range`, or a dist-tag `Tag`. Returns `None` for anything that
//! isn't one of those (git URLs, `file:` paths, `link:` paths,
//! `workspace:` protocol entries) — those have no place in the
//! version-picker tie-break table the preferred-versions map feeds.

use node_semver::{Range, Version};
use pacquet_resolving_resolver_base::VersionSelectorType;

/// Classify a manifest spec the same way upstream's loose
/// `getVersionSelectorType` does.
pub fn get_version_selector_type(spec: &str) -> Option<VersionSelectorType> {
    if spec.parse::<Version>().is_ok() {
        return Some(VersionSelectorType::Version);
    }
    if spec.parse::<Range>().is_ok() {
        return Some(VersionSelectorType::Range);
    }
    if is_uri_component_safe(spec) {
        return Some(VersionSelectorType::Tag);
    }
    None
}

/// `encodeURIComponent(s) === s` in the upstream check. JavaScript's
/// `encodeURIComponent` percent-encodes everything outside the
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )` set, so the comparison is true
/// exactly when every character of `s` is in that set.
fn is_uri_component_safe(spec: &str) -> bool {
    !spec.is_empty()
        && spec.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        })
}

#[cfg(test)]
mod tests {
    use pacquet_resolving_resolver_base::VersionSelectorType;
    use pretty_assertions::assert_eq;

    use super::get_version_selector_type;

    #[test]
    fn classifies_exact_version_as_version() {
        assert_eq!(get_version_selector_type("1.0.0"), Some(VersionSelectorType::Version));
        assert_eq!(get_version_selector_type("1.2.3-beta.1"), Some(VersionSelectorType::Version));
    }

    #[test]
    fn classifies_semver_range_as_range() {
        assert_eq!(get_version_selector_type("^1.0.0"), Some(VersionSelectorType::Range));
        assert_eq!(get_version_selector_type(">=1.0.0 <2.0.0"), Some(VersionSelectorType::Range));
        assert_eq!(get_version_selector_type("1.x"), Some(VersionSelectorType::Range));
    }

    #[test]
    fn classifies_url_safe_string_as_tag() {
        assert_eq!(get_version_selector_type("latest"), Some(VersionSelectorType::Tag));
        assert_eq!(get_version_selector_type("next"), Some(VersionSelectorType::Tag));
        assert_eq!(get_version_selector_type("beta-rc.1"), Some(VersionSelectorType::Tag));
    }

    #[test]
    fn rejects_unknown_specs() {
        assert_eq!(get_version_selector_type("git+ssh://example.com/repo.git"), None);
        assert_eq!(get_version_selector_type("file:./local-path"), None);
        assert_eq!(get_version_selector_type("workspace:*"), None);
        assert_eq!(get_version_selector_type("npm:other@^1"), None);
        assert_eq!(get_version_selector_type(""), None);
    }
}
