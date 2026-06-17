use super::{SnapshotDepRef, looks_like_alias};
use crate::{PkgName, PkgNameVerPeer, PkgVerPeer};
use pretty_assertions::assert_eq;

fn pkg_name(text: &str) -> PkgName {
    PkgName::parse(text).unwrap()
}

fn ver_peer(text: &str) -> PkgVerPeer {
    text.parse().unwrap()
}

fn key(text: &str) -> PkgNameVerPeer {
    text.parse().unwrap()
}

#[test]
fn parse_plain_version() {
    let dep: SnapshotDepRef = "5.1.2".parse().unwrap();
    assert_eq!(dep, SnapshotDepRef::Plain(ver_peer("5.1.2")));
}

#[test]
fn parse_plain_with_peer_suffix() {
    let dep: SnapshotDepRef = "17.0.2(react@17.0.2)".parse().unwrap();
    assert_eq!(dep, SnapshotDepRef::Plain(ver_peer("17.0.2(react@17.0.2)")));
}

#[test]
fn parse_alias_unscoped_target() {
    let dep: SnapshotDepRef = "string-width@4.2.3".parse().unwrap();
    assert_eq!(dep, SnapshotDepRef::Alias(key("string-width@4.2.3")));
}

#[test]
fn parse_alias_scoped_target() {
    let dep: SnapshotDepRef = "@types/react@17.0.49".parse().unwrap();
    assert_eq!(dep, SnapshotDepRef::Alias(key("@types/react@17.0.49")));
}

#[test]
fn parse_alias_with_peer_suffix() {
    let dep: SnapshotDepRef = "react-dom@17.0.2(react@17.0.2)".parse().unwrap();
    assert_eq!(dep, SnapshotDepRef::Alias(key("react-dom@17.0.2(react@17.0.2)")));
}

#[test]
fn resolve_plain_uses_alias_key_as_target_name() {
    let dep: SnapshotDepRef = "5.1.2".parse().unwrap();
    let resolved = dep.resolve(&pkg_name("string-width")).expect("plain resolves");
    assert_eq!(resolved.to_string(), "string-width@5.1.2");
}

#[test]
fn resolve_alias_uses_alias_target_name_not_key() {
    let dep: SnapshotDepRef = "string-width@4.2.3".parse().unwrap();
    let resolved = dep.resolve(&pkg_name("string-width-cjs")).expect("alias resolves");
    assert_eq!(resolved.to_string(), "string-width@4.2.3");
}

#[test]
fn resolve_link_returns_none() {
    let dep: SnapshotDepRef = "link:packages/c".parse().unwrap();
    assert_eq!(dep.resolve(&pkg_name("c")), None);
}

#[test]
fn display_roundtrip() {
    for input in [
        "5.1.2",
        "17.0.2(react@17.0.2)",
        "string-width@4.2.3",
        "@types/react@17.0.49",
        "react-dom@17.0.2(react@17.0.2)",
        "link:packages/c",
        "link:../sibling",
    ] {
        let dep: SnapshotDepRef = input.parse().unwrap();
        assert_eq!(dep.to_string(), input);
    }
}

#[test]
fn deserialize_ok() {
    for (yaml, expected) in [
        ("5.1.2", "5.1.2"),
        ("string-width@4.2.3", "string-width@4.2.3"),
        (r#""17.0.2(react@17.0.2)""#, "17.0.2(react@17.0.2)"),
        ("link:packages/c", "link:packages/c"),
    ] {
        let dep: SnapshotDepRef = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(dep.to_string(), expected);
    }
}

#[test]
fn parse_link_workspace_path() {
    let dep: SnapshotDepRef = "link:packages/c".parse().unwrap();
    assert_eq!(dep, SnapshotDepRef::Link("packages/c".to_string()));
    assert_eq!(dep.as_link_target(), Some("packages/c"));
}

#[test]
fn looks_like_alias_rules() {
    for (input, expected) in [
        ("5.1.2", false),
        ("17.0.2(react@17.0.2)", false),
        ("string-width@4.2.3", true),
        ("@types/react@17.0.49", true),
        ("react-dom@17.0.2(react@17.0.2)", true),
        // protocol-like refs are not aliases
        ("link:../foo", false),
        ("workspace:*", false),
    ] {
        eprintln!("CASE: {input:?}");
        assert_eq!(looks_like_alias(input), expected);
    }
}

#[test]
fn ver_peer_returns_inner_version_for_each_variant() {
    let plain: SnapshotDepRef = "17.0.2(react@17.0.2)".parse().unwrap();
    assert_eq!(plain.ver_peer().map(ToString::to_string), Some("17.0.2(react@17.0.2)".to_string()));

    let alias: SnapshotDepRef = "react-dom@17.0.2(react@17.0.2)".parse().unwrap();
    assert_eq!(alias.ver_peer().map(ToString::to_string), Some("17.0.2(react@17.0.2)".to_string()));

    let link: SnapshotDepRef = "link:packages/c".parse().unwrap();
    assert_eq!(link.ver_peer(), None);
}

#[test]
fn from_pkg_ver_peer_produces_plain_variant() {
    let ver = ver_peer("17.0.2(react@17.0.2)");
    let dep: SnapshotDepRef = ver.clone().into();
    assert_eq!(dep, SnapshotDepRef::Plain(ver));
}
