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

    assert!(package.versions.contains_key("9.9.9"));
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

#[test]
fn is_deprecated_probes_without_hydrating() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}},
                "1.1.0": {"name": "foo", "version": "1.1.0", "deprecated": "use 2.x", "dist": {"integrity": "sha512-b", "tarball": "https://r/foo-1.1.0.tgz"}},
                "1.2.0": {"name": "foo", "version": "1.2.0", "deprecated": false, "dist": {"integrity": "sha512-c", "tarball": "https://r/foo-1.2.0.tgz"}},
                "1.3.0": {"name": "foo", "version": "1.3.0", "deprecated": true, "dist": {"integrity": "sha512-d", "tarball": "https://r/foo-1.3.0.tgz"}},
                "1.4.0": {"name": "foo", "version": "1.4.0", "deprecated": "", "dist": {"integrity": "sha512-e", "tarball": "https://r/foo-1.4.0.tgz"}},
                "1.9.0": {"corrupt": "fragment", "deprecated": 1}
            }
        }"#,
    );

    assert!(!package.versions.is_deprecated("1.0.0"));
    assert!(package.versions.is_deprecated("1.1.0"));
    assert!(!package.versions.is_deprecated("1.2.0"));
    assert!(package.versions.is_deprecated("1.3.0"));
    assert!(package.versions.is_deprecated("1.4.0"));
    assert!(!package.versions.is_deprecated("9.9.9"));
    assert!(!package.versions.is_deprecated("1.9.0"));

    // The probe must agree with the hydrated field on every slot it
    // can hydrate, and must not have hydrated anything itself: `get`
    // still parses fresh (no cached Arc identity from the probe).
    for version in ["1.0.0", "1.1.0", "1.2.0", "1.3.0", "1.4.0"] {
        let manifest = package.versions.get(version).expect("hydrate");
        assert_eq!(
            package.versions.is_deprecated(version),
            manifest.deprecated.is_some(),
            "probe vs hydrated disagree for {version}",
        );
    }
}

#[test]
fn is_deprecated_ignores_unrelated_key_text() {
    let package = parse_package(
        r#"{
            "name": "foo",
            "dist-tags": {},
            "versions": {
                "1.0.0": {"name": "foo", "version": "1.0.0", "dependencies": {"deprecated": "^0.0.2"}, "dist": {"integrity": "sha512-a", "tarball": "https://r/foo-1.0.0.tgz"}}
            }
        }"#,
    );
    assert!(!package.versions.is_deprecated("1.0.0"));
}
