//! Classifies a manifest specifier as a concrete `Version`, a semver
//! `Range`, or a dist-tag `Tag`. Returns `None` for anything that
//! isn't one of those — those have no place in the version-picker
//! tie-break table the preferred-versions map feeds.

use node_semver::{Range, Version};
use pacquet_resolving_resolver_base::VersionSelectorType;

/// Classify a manifest spec as `Version`, `Range`, or `Tag`, using the
/// loose precedence that tries an exact version first, then a range,
/// then a dist-tag.
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

/// A spec is a valid dist-tag when `encodeURIComponent(s) === s`.
/// JavaScript's `encodeURIComponent` percent-encodes everything outside
/// the `A-Z a-z 0-9 - _ . ! ~ * ' ( )` set, so the comparison is true
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
