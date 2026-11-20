//! Pacquet's [`normalize_bundled_manifest`] picks the subset of
//! `package.json` fields downstream install code reads (bin lookup,
//! peer extraction, build-script detection) and narrows `scripts` to
//! the lifecycle hooks. Two cases are intentionally NOT covered:
//! `semver.clean` normalization (pacquet keeps version verbatim, per
//! the function's doc comment) and the missing-version default of
//! `0.0.0` (pacquet leaves the field absent rather than synthesizing
//! one).

use crate::normalize_bundled_manifest;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn returns_none_for_empty_manifest() {
    assert_eq!(normalize_bundled_manifest(&json!({})), None);
}

#[test]
fn returns_none_for_non_object() {
    assert_eq!(normalize_bundled_manifest(&json!("not an object")), None);
    assert_eq!(normalize_bundled_manifest(&json!(null)), None);
    assert_eq!(normalize_bundled_manifest(&json!(42)), None);
}

#[test]
fn returns_none_when_manifest_has_only_excluded_fields() {
    assert_eq!(
        normalize_bundled_manifest(&json!({
            "description": "a package",
            "keywords": ["test"],
            "license": "MIT",
            "author": "test",
            "repository": "test/test",
        })),
        None,
    );
}

#[test]
fn picks_included_fields_and_excludes_others() {
    let result = normalize_bundled_manifest(&json!({
        "name": "foo",
        "version": "1.0.0",
        "description": "should be excluded",
        "license": "MIT",
        "bin": { "foo": "./bin/foo.js" },
        "engines": { "node": ">=18" },
        "cpu": ["x64"],
        "os": ["linux"],
        "libc": ["glibc"],
        "dependencies": { "bar": "^1.0.0" },
        "devDependencies": { "qux": "^3.0.0" },
        "optionalDependencies": { "baz": "^2.0.0" },
        "peerDependencies": { "react": "^18" },
        "peerDependenciesMeta": { "react": { "optional": true } },
        "bundledDependencies": ["bar"],
        "directories": { "bin": "./bin" },
    }))
    .expect("non-empty pick");
    let map = result.as_object().expect("object");
    assert_eq!(map.get("name").and_then(|v| v.as_str()), Some("foo"));
    assert_eq!(map.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
    assert_eq!(map.get("bin"), Some(&json!({ "foo": "./bin/foo.js" })));
    assert_eq!(map.get("engines"), Some(&json!({ "node": ">=18" })));
    assert_eq!(map.get("cpu"), Some(&json!(["x64"])));
    assert_eq!(map.get("os"), Some(&json!(["linux"])));
    assert_eq!(map.get("libc"), Some(&json!(["glibc"])));
    assert_eq!(map.get("dependencies"), Some(&json!({ "bar": "^1.0.0" })));
    assert_eq!(map.get("devDependencies"), Some(&json!({ "qux": "^3.0.0" })));
    assert_eq!(map.get("optionalDependencies"), Some(&json!({ "baz": "^2.0.0" })));
    assert_eq!(map.get("peerDependencies"), Some(&json!({ "react": "^18" })));
    assert_eq!(map.get("peerDependenciesMeta"), Some(&json!({ "react": { "optional": true } })));
    assert_eq!(map.get("bundledDependencies"), Some(&json!(["bar"])));
    assert_eq!(map.get("directories"), Some(&json!({ "bin": "./bin" })));
    // Excluded fields stay out.
    assert!(map.get("description").is_none());
    assert!(map.get("license").is_none());
    assert!(map.get("keywords").is_none());
}

#[test]
fn only_picks_lifecycle_scripts_not_all_scripts() {
    let result = normalize_bundled_manifest(&json!({
        "name": "foo",
        "version": "1.0.0",
        "scripts": {
            "preinstall": "echo pre",
            "install": "echo install",
            "postinstall": "echo post",
            "test": "jest",
            "build": "tsc",
            "start": "node index.js",
            "prepare": "tsc",
        },
    }))
    .expect("non-empty pick");
    assert_eq!(
        result.get("scripts").expect("scripts present"),
        &json!({
            "preinstall": "echo pre",
            "install": "echo install",
            "postinstall": "echo post",
        }),
    );
}

#[test]
fn omits_scripts_key_when_no_lifecycle_scripts_exist() {
    let result = normalize_bundled_manifest(&json!({
        "name": "foo",
        "version": "1.0.0",
        "scripts": {
            "test": "jest",
            "build": "tsc",
        },
    }))
    .expect("non-empty pick");
    assert!(
        result.get("scripts").is_none(),
        "scripts key must be absent when no lifecycle hook is present",
    );
}

/// `null` and `undefined` fields are skipped. Rust's
/// [`serde_json::Value`] has no `undefined`, but JSON `null`
/// reaches the picker as [`serde_json::Value::Null`] and must be
/// filtered out the same way.
#[test]
fn skips_null_fields() {
    let result = normalize_bundled_manifest(&json!({
        "name": "foo",
        "version": "1.0.0",
        "bin": null,
        "engines": null,
    }))
    .expect("non-empty pick");
    assert!(result.get("bin").is_none(), "null `bin` must be dropped");
    assert!(result.get("engines").is_none(), "null `engines` must be dropped");
    assert_eq!(result.get("name").and_then(|v| v.as_str()), Some("foo"));
    assert_eq!(result.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
}

/// The bundled manifest is downstream-fed into
/// `extract_peer_dependencies` and `extract_children`; dropping
/// `peerDependenciesMeta` or `optionalDependencies` here would
/// replicate the pnpm/pnpm#11934 resolver-side bug on the
/// install-side. Pin the keys explicitly.
#[test]
fn preserves_optional_dependencies_and_peer_dependencies_meta_keys() {
    let result = normalize_bundled_manifest(&json!({
        "name": "consumer",
        "version": "1.0.0",
        "optionalDependencies": { "sharp": "^0.34.0" },
        "peerDependenciesMeta": {
            "@vercel/kv": { "optional": true },
            "ioredis": { "optional": true },
        },
    }))
    .expect("non-empty pick");
    assert_eq!(result.get("optionalDependencies"), Some(&json!({ "sharp": "^0.34.0" })));
    assert_eq!(
        result.get("peerDependenciesMeta"),
        Some(&json!({
            "@vercel/kv": { "optional": true },
            "ioredis": { "optional": true },
        })),
    );
}
