use super::drop_conflicted_versions;
use serde_json::json;
use std::collections::HashSet;

#[test]
fn drop_conflicted_versions_removes_only_the_lost_versions() {
    let mut journaled = json!({
        "versions": {
            "1.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-1.0.0.tgz" } },
            "2.0.0": { "dist": { "tarball": "http://host/pkg/-/pkg-2.0.0.tgz" } },
            // A version with no resolvable tarball basename is kept as-is.
            "3.0.0": { "dist": {} },
        }
    });
    let conflicted: HashSet<&str> = std::iter::once("pkg-1.0.0.tgz").collect();

    drop_conflicted_versions(&mut journaled, &conflicted);

    let versions = journaled["versions"].as_object().unwrap();
    assert!(!versions.contains_key("1.0.0"));
    assert!(versions.contains_key("2.0.0"));
    assert!(versions.contains_key("3.0.0"));
}

#[test]
fn drop_conflicted_versions_tolerates_a_missing_versions_map() {
    let mut journaled = json!({ "name": "pkg" });
    let conflicted: HashSet<&str> = std::iter::once("pkg-1.0.0.tgz").collect();
    drop_conflicted_versions(&mut journaled, &conflicted);
    assert_eq!(journaled, json!({ "name": "pkg" }));
}
