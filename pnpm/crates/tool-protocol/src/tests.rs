use super::{is_tool_specifier, strip_tool_protocol, tool_specifiers_equal};
use pretty_assertions::assert_eq;

#[test]
fn strip_accepts_both_spellings() {
    assert_eq!(strip_tool_protocol("tool:22"), Some("22"));
    assert_eq!(strip_tool_protocol("runtime:^1.3.0"), Some("^1.3.0"));
    assert_eq!(strip_tool_protocol("tool:"), Some(""));
}

#[test]
fn other_protocols_are_not_tools() {
    for specifier in ["22", "npm:tool@1", "workspace:*", "file:tool:x", "tools:1"] {
        assert_eq!(strip_tool_protocol(specifier), None, "{specifier}");
        assert!(!is_tool_specifier(specifier), "{specifier}");
    }
}

#[test]
fn spellings_compare_equal() {
    assert!(tool_specifiers_equal("tool:22", "runtime:22"));
    assert!(tool_specifiers_equal("runtime:22", "tool:22"));
    assert!(tool_specifiers_equal("tool:22", "tool:22"));
    assert!(!tool_specifiers_equal("tool:22", "runtime:24"));
    assert!(!tool_specifiers_equal("tool:22", "22"));
}

#[test]
fn package_managers_come_from_their_own_sources() {
    use crate::{PackageManagerSource, package_manager_source};
    assert_eq!(package_manager_source("npm", "10"), Some(PackageManagerSource::Registry("npm")));
    assert_eq!(
        package_manager_source("yarn", "1.22.22"),
        Some(PackageManagerSource::Registry("yarn")),
    );
    assert_eq!(
        package_manager_source("yarn", ">=2"),
        Some(PackageManagerSource::Registry("@yarnpkg/cli-dist")),
    );
    assert_eq!(
        package_manager_source("yarn", "latest"),
        Some(PackageManagerSource::Registry("@yarnpkg/cli-dist")),
    );
    assert_eq!(package_manager_source("yarn", "6.0.0"), Some(PackageManagerSource::Archive));
    assert_eq!(package_manager_source("bun", "1.3.0"), Some(PackageManagerSource::Archive));
    assert_eq!(package_manager_source("cnpm", "1"), None);
}
