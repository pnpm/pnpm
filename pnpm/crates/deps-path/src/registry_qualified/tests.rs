use super::{is_well_formed_registry_name, parse_registry_qualified_version};
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
