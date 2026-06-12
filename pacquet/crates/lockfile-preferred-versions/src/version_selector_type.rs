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
#[must_use]
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
        && spec.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        })
}

#[cfg(test)]
mod tests;
