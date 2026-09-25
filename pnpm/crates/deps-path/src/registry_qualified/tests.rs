use super::{
    is_well_formed_registry_name, parse_registry_qualified_version, shadows_reserved_version_prefix,
};
use node_semver::Version;

#[test]
fn parses_registry_qualified_versions() {
    let (name, version) = parse_registry_qualified_version("work:1.0.0").unwrap();
    assert_eq!(name, "work");
    assert_eq!(version, Version::parse("1.0.0").unwrap());

    let (name, version) = parse_registry_qualified_version("gh:2.1.0-beta.1").unwrap();
    assert_eq!(name, "gh");
    assert_eq!(version, Version::parse("2.1.0-beta.1").unwrap());
}

#[test]
fn rejects_reserved_prefixes_and_non_semver() {
    assert!(parse_registry_qualified_version("file:1.0.0").is_none());
    assert!(parse_registry_qualified_version("runtime:24.0.0").is_none());
    assert!(parse_registry_qualified_version("1.0.0").is_none());
    assert!(parse_registry_qualified_version("work:^1.0.0").is_none());
    assert!(parse_registry_qualified_version("9work:1.0.0").is_none());
    assert!(parse_registry_qualified_version(":1.0.0").is_none());
}

#[test]
fn well_formed_registry_names() {
    assert!(is_well_formed_registry_name("work"));
    assert!(is_well_formed_registry_name("my-registry.v2"));
    assert!(!is_well_formed_registry_name("9work"));
    assert!(!is_well_formed_registry_name("no colons"));
    assert!(!is_well_formed_registry_name(""));
}

/// A dep path carries the prefix pnpm wrote, so reading one back matches it
/// exactly. An alias is rejected in any case only for a prefix a selector
/// may spell in any case.
#[test]
fn an_alias_shadows_a_case_insensitive_prefix_in_any_case() {
    assert!(shadows_reserved_version_prefix("pkg"));
    assert!(shadows_reserved_version_prefix("PKG"));
    assert!(shadows_reserved_version_prefix("Pkg"));
    assert!(shadows_reserved_version_prefix("npm"));
    // `npm:` is read exactly, so `Npm:lodash` names a registry called `Npm`.
    assert!(!shadows_reserved_version_prefix("Npm"));
    assert!(!shadows_reserved_version_prefix("work"));
    assert!(!shadows_reserved_version_prefix("pkgs"));
    assert_eq!(parse_registry_qualified_version("PKG:1.0.0").map(|(name, _)| name), Some("PKG"));
}
