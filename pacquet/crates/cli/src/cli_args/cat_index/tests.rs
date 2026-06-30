use pacquet_lockfile::ResolvedDependencySpec;
use serde_json::json;

use super::{MAX_JSON_SORT_DEPTH, request_matches_dependency, sort_deep_keys};

#[test]
fn request_does_not_reuse_lockfile_entry_for_different_exact_version() {
    let dependency = ResolvedDependencySpec {
        specifier: "^3.1.0".to_string(),
        version: "3.1.2".parse().expect("parse importer dep version"),
    };

    assert!(request_matches_dependency("bytes", Some("^3.1.0"), &dependency, "bytes@3.1.2"));
    assert!(request_matches_dependency("bytes", Some("3.1.2"), &dependency, "bytes@3.1.2"));
    assert!(!request_matches_dependency("bytes", Some("3.1.1"), &dependency, "bytes@3.1.2"));
}

#[test]
fn sort_deep_keys_rejects_too_deep_json() {
    let mut value = json!(null);
    for _ in 0..=MAX_JSON_SORT_DEPTH {
        value = json!({ "child": value });
    }

    let error = sort_deep_keys(&mut value, 0).expect_err("deep JSON is rejected");
    assert!(error.to_string().contains("nested too deeply"));
}

#[test]
fn sort_deep_keys_sorts_nested_objects_deterministically() {
    let mut value = json!({
        "z": 1,
        "a": {
            "d": 4,
            "b": 2,
            "c": [{ "y": 2, "x": 1 }],
        },
        "m": [{ "beta": 2, "alpha": 1 }],
    });

    sort_deep_keys(&mut value, 0).expect("sort JSON keys");

    assert_eq!(object_keys(&value), vec!["a", "m", "z"]);

    let nested = value.get("a").expect("has nested object");
    assert_eq!(object_keys(nested), vec!["b", "c", "d"]);

    let array_object = nested
        .get("c")
        .expect("has nested array")
        .as_array()
        .expect("nested value is an array")
        .first()
        .expect("nested array has an object");
    assert_eq!(object_keys(array_object), vec!["x", "y"]);

    let root_array_object = value
        .get("m")
        .expect("has root array")
        .as_array()
        .expect("root value is an array")
        .first()
        .expect("root array has an object");
    assert_eq!(object_keys(root_array_object), vec!["alpha", "beta"]);
}

fn object_keys(value: &serde_json::Value) -> Vec<&str> {
    value.as_object().expect("value is an object").keys().map(String::as_str).collect()
}
