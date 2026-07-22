use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::{
    Packument, highest_version, registry_origin, rev_str, tarball_pathname, versions_matching_range,
};

fn versions(keys: &[&str]) -> BTreeMap<String, Value> {
    keys.iter().map(|key| ((*key).to_string(), json!({}))).collect()
}

#[test]
fn rev_str_falls_back_to_the_undefined_literal() {
    assert_eq!(rev_str(Some("3-abc")), "3-abc");
    // A packument without _rev renders like the TypeScript template string.
    assert_eq!(rev_str(None), "undefined");
}

#[test]
fn versions_matching_range_mirrors_semver_satisfies() {
    let versions = versions(&["1.0.0", "1.5.0", "2.0.0"]);
    assert_eq!(versions_matching_range(&versions, "1.0.0"), ["1.0.0"]);
    assert_eq!(versions_matching_range(&versions, "^1.0.0"), ["1.0.0", "1.5.0"]);
    assert_eq!(versions_matching_range(&versions, ">=1.0.0"), ["1.0.0", "1.5.0", "2.0.0"]);
    assert!(versions_matching_range(&versions, "9.9.9").is_empty(), "no match yields empty");
    assert!(versions_matching_range(&versions, "not a range").is_empty(), "junk matches nothing");
}

#[test]
fn highest_version_picks_the_new_latest() {
    assert_eq!(
        highest_version(&versions(&["1.0.0", "10.0.0", "9.0.0"])).as_deref(),
        Some("10.0.0"),
    );
    assert_eq!(
        highest_version(&versions(&["1.0.0", "1.0.1-beta.1"])).as_deref(),
        Some("1.0.1-beta.1"),
    );
    assert_eq!(highest_version(&versions(&[])), None);
}

#[test]
fn registry_origin_drops_the_registry_path() {
    let origin = registry_origin("https://registry.example.com:8443/npm/").expect("an origin");
    assert_eq!(origin, "https://registry.example.com:8443");
}

#[test]
fn tarball_pathname_strips_the_registry_path_prefix() {
    // A registry at the host root keeps the tarball path as is.
    let pathname = tarball_pathname(
        "https://registry.example.com/pkg/-/pkg-1.0.0.tgz",
        "https://registry.example.com/",
    )
    .expect("a pathname");
    assert_eq!(pathname, "pkg/-/pkg-1.0.0.tgz");

    // A registry mounted under a path is stripped from the tarball path.
    let pathname = tarball_pathname(
        "https://registry.example.com/npm/pkg/-/pkg-1.0.0.tgz",
        "https://registry.example.com/npm/",
    )
    .expect("a pathname");
    assert_eq!(pathname, "pkg/-/pkg-1.0.0.tgz");
}

/// The packument round-trips unknown fields and drops the `CouchDB` metadata
/// keys the way the PUT body requires.
#[test]
fn packument_round_trips_unknown_fields() {
    let raw = json!({
        "name": "pkg",
        "_rev": "3-abc",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "dist": { "tarball": "https://x/pkg-1.0.0.tgz" } } },
        "readme": "hello",
        "_revisions": { "start": 3 },
        "_attachments": {},
    });
    let mut packument: Packument = serde_json::from_value(raw).expect("a packument deserializes");
    assert_eq!(packument.rev.as_deref(), Some("3-abc"));

    packument.other.remove("_revisions");
    packument.other.remove("_attachments");
    let serialized = serde_json::to_value(&packument).expect("a packument serializes");
    assert_eq!(serialized.get("readme"), Some(&json!("hello")), "unknown fields survive");
    assert!(serialized.get("_revisions").is_none(), "couchdb metadata is dropped");
    assert!(serialized.get("_attachments").is_none(), "couchdb metadata is dropped");
}
