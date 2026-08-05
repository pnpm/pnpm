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
