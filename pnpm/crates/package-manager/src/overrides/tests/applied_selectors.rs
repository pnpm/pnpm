use super::{
    VersionsOverrider,
    manifest_from_value,
    parsed,
    undeclared,
};
use serde_json::json;
use std::{
    collections::HashSet,
    path::Path,
};

#[test]
fn records_each_override_that_rewrote_an_edge() {
    let overrides = parsed(&[
        ("foo", "1.0.0"),
        ("parent>bar", "1.5.0"),
        ("delete-me", "-"),
        ("never-matches", "9.9.9"),
    ]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": {
            "foo": "^1.0.0",
            "delete-me": "^3.0.0",
            "bar": "^1.0.0",
        },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(
        overrider.applied_selectors(),
        HashSet::from(["delete-me".to_string(), "foo".to_string(), "parent>bar".to_string()]),
    );
}

#[test]
fn stays_empty_when_no_override_matches() {
    let overrides = parsed(&[("foo", "1.0.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "bar": "^1.0.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(overrider.applied_selectors(), HashSet::new());
}

#[test]
fn records_overrides_applied_to_peer_and_undeclared_edges() {
    let overrides = parsed(&[("react", "18.2.0"), ("lodash", "4.17.21")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-lib",
        "version": "1.0.0",
        "peerDependencies": { "react": "^18.0.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));
    assert_eq!(overrider.applied_selectors(), HashSet::from(["react".to_string()]));

    assert_eq!(undeclared(&overrider, "lodash", "^4.0.0"), Some("4.17.21".to_string()));
    assert_eq!(
        overrider.applied_selectors(),
        HashSet::from(["lodash".to_string(), "react".to_string()]),
    );
}

#[test]
fn never_records_convergence_overrides() {
    let overrides = parsed(&[("foo@", "1.2.0")]);
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));

    let mut manifest = manifest_from_value(json!({
        "name": "my-app",
        "version": "1.0.0",
        "dependencies": { "foo": "^1.0.0" },
    }));
    overrider.apply(&mut manifest, Some(Path::new("/workspace")));

    assert_eq!(overrider.applied_selectors(), HashSet::new());
}
