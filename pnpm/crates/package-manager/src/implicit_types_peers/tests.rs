use super::add_implicit_types_peers;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::Arc;

fn transform(manifest: Value) -> Value {
    Value::clone(&add_implicit_types_peers(Arc::new(manifest)))
}

#[test]
fn adds_optional_types_peer_for_each_peer() {
    let manifest = json!({
        "name": "next",
        "peerDependencies": { "react": "^19.0.0", "@babel/core": "^7.0.0" },
    });

    assert_eq!(
        transform(manifest),
        json!({
            "name": "next",
            "peerDependencies": {
                "react": "^19.0.0",
                "@babel/core": "^7.0.0",
                "@types/react": "*",
                "@types/babel__core": "*",
            },
            "peerDependenciesMeta": {
                "@types/react": { "optional": true },
                "@types/babel__core": { "optional": true },
            },
        }),
    );
}

#[test]
fn keeps_types_the_package_declares() {
    let manifest = json!({
        "name": "pkg",
        "dependencies": { "@types/a": "1.0.0" },
        "optionalDependencies": { "@types/b": "1.0.0" },
        "peerDependencies": { "a": "1", "b": "1", "c": "1", "d": "1", "@types/c": "^2" },
        "peerDependenciesMeta": { "@types/d": { "optional": false } },
    });

    assert_eq!(transform(manifest.clone()), manifest);
}

#[test]
fn skips_peers_that_are_types_packages() {
    let manifest = json!({ "name": "pkg", "peerDependencies": { "@types/node": "*" } });

    assert_eq!(transform(manifest.clone()), manifest);
}

#[test]
fn keeps_the_shared_manifest_when_nothing_is_added() {
    let manifest = Arc::new(json!({ "name": "pkg", "dependencies": { "react": "19" } }));

    let transformed = add_implicit_types_peers(Arc::clone(&manifest));

    assert!(Arc::ptr_eq(&manifest, &transformed));
}
