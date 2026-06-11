use std::collections::HashMap;

use crate::{Package, PackageVersion};

fn parse_package(json: &str) -> Package {
    serde_json::from_str(json).expect("parse package")
}

#[test]
fn hydrates_only_requested_versions_and_caches_them() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {"latest": "2.0.0"},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "2.0.0": {"name": "foo", "version": "2.0.0", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-2.0.0.tgz"}}
            }
        }"#,
    );

    assert_eq!(package.versions.len(), 2);
    assert!(package.versions.contains_key("1.0.0"));
    let picked = package.versions.get("2.0.0").expect("hydrate 2.0.0");
    assert_eq!(picked.version.to_string(), "2.0.0");
    // Second lookup returns the cached Arc.
    let again = package.versions.get("2.0.0").expect("cached 2.0.0");
    assert!(std::sync::Arc::ptr_eq(&picked, &again));
}

#[test]
fn undecodable_fragment_behaves_as_absent() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "9.9.9": {"this is": "not a version manifest"}
            }
        }"#,
    );

    // The key is listed (key scans never hydrate)...
    assert!(package.versions.contains_key("9.9.9"));
    // ...but hydration fails closed to "absent" instead of erroring.
    assert!(package.versions.get("9.9.9").is_none());
    assert!(package.versions.get("1.0.0").is_some());
    assert_eq!(package.versions.iter().count(), 1);
}

#[test]
fn serializes_raw_fragments_verbatim() {
    let json = r#"{
        "name": "foo",
        "dist-tags": {},
        "versions": {
            "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}, "extraKeyTheStructDoesNotType": [1, 2, {"deep": true}]}
        }
    }"#;
    let package = parse_package(json);
    let round_tripped = serde_json::to_string(&package).expect("serialize package");
    let reparsed: serde_json::Value = serde_json::from_str(&round_tripped).unwrap();
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(reparsed["versions"], original["versions"]);
}

#[test]
fn eager_construction_from_typed_manifests_round_trips() {
    let manifest: PackageVersion = serde_json::from_str(
        r#"{"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}"#,
    )
    .unwrap();
    let versions: crate::PackageVersions = HashMap::from([("1.0.0".to_string(), manifest)]).into();
    assert_eq!(versions.get("1.0.0").unwrap().version.to_string(), "1.0.0");
    let json = serde_json::to_string(&versions).unwrap();
    assert!(json.contains(r#""1.0.0""#));
}

#[test]
fn filtered_keeps_slots_without_hydration() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "2.0.0": {"name": "foo", "version": "2.0.0", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-2.0.0.tgz"}}
            }
        }"#,
    );
    let filtered = package.versions.filtered(|version| version == "1.0.0");
    assert_eq!(filtered.len(), 1);
    assert!(filtered.get("1.0.0").is_some());
}

/// `pinned_version` walks satisfying candidates from highest to
/// lowest, so an undecodable winner falls back to the next match
/// instead of reporting no match for the whole range.
#[test]
fn pinned_version_falls_back_past_undecodable_highest() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "1.9.0": {"corrupt": "fragment"}
            }
        }"#,
    );
    let pinned = package.pinned_version("^1.0.0").expect("fall back to 1.0.0");
    assert_eq!(pinned.version.to_string(), "1.0.0");
}

/// `latest` degrades to `None` on registry data it can't use — a
/// dangling dist-tag or an undecodable manifest — instead of
/// panicking.
#[test]
fn latest_returns_none_for_dangling_or_undecodable_tag() {
    let undecodable = parse_package(
        r#"{"name": "foo", "dist-tags": {"latest": "2.0.0"}, "versions": {"2.0.0": {"corrupt": true}}}"#,
    );
    assert!(undecodable.latest().is_none());
    let dangling =
        parse_package(r#"{"name": "foo", "dist-tags": {"latest": "9.9.9"}, "versions": {}}"#);
    assert!(dangling.latest().is_none());
}
