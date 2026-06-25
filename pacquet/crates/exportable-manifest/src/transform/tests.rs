use super::{TransformError, transform};
use serde_json::{Value, json};

fn run(mut manifest: Value) -> Result<Value, TransformError> {
    let object = manifest.as_object_mut().unwrap();
    transform(object)?;
    Ok(manifest)
}

#[test]
fn missing_name_is_rejected() {
    let err = run(json!({ "version": "1.0.0" })).unwrap_err();
    assert_eq!(err, TransformError::MissingRequiredField { field: "name" });
}

#[test]
fn missing_version_is_rejected() {
    let err = run(json!({ "name": "foo" })).unwrap_err();
    assert_eq!(err, TransformError::MissingRequiredField { field: "version" });
}

#[test]
fn empty_string_name_is_rejected() {
    let err = run(json!({ "name": "", "version": "1.0.0" })).unwrap_err();
    assert_eq!(err, TransformError::MissingRequiredField { field: "name" });
}

#[test]
fn falsy_version_is_rejected() {
    for falsy in [json!(0), json!(false), json!(null)] {
        let err = run(json!({ "name": "foo", "version": falsy })).unwrap_err();
        assert_eq!(err, TransformError::MissingRequiredField { field: "version" });
    }
}

#[test]
fn non_string_truthy_required_fields_pass() {
    // pnpm's `if (!manifest.name)` accepts any truthy value, so a
    // numeric `name` / `version` must survive validation just as it
    // does upstream rather than be rejected as "missing".
    let out = run(json!({ "name": 1, "version": 2 })).unwrap();
    assert_eq!(out["name"], json!(1));
    assert_eq!(out["version"], json!(2));
}

#[test]
fn string_bin_becomes_object_under_unscoped_name() {
    let out = run(json!({ "name": "foo", "version": "1.0.0", "bin": "cli.js" })).unwrap();
    assert_eq!(out["bin"], json!({ "foo": "cli.js" }));
}

#[test]
fn string_bin_strips_scope_for_command_name() {
    let out = run(json!({ "name": "@scope/foo", "version": "1.0.0", "bin": "cli.js" })).unwrap();
    assert_eq!(out["bin"], json!({ "foo": "cli.js" }));
}

#[test]
fn object_bin_is_left_untouched() {
    let out = run(json!({
        "name": "foo",
        "version": "1.0.0",
        "bin": { "a": "a.js", "b": "b.js" },
    }))
    .unwrap();
    assert_eq!(out["bin"], json!({ "a": "a.js", "b": "b.js" }));
}

#[test]
fn scoped_name_without_slash_and_string_bin_is_rejected() {
    let err = run(json!({ "name": "@foo", "version": "1.0.0", "bin": "cli.js" })).unwrap_err();
    assert_eq!(err, TransformError::InvalidScopedPackageName { invalid_name: "@foo".to_string() });
}

#[test]
fn peer_dependencies_meta_gets_explicit_optional() {
    let out = run(json!({
        "name": "foo",
        "version": "1.0.0",
        "peerDependenciesMeta": { "react": {}, "vue": { "optional": true } },
    }))
    .unwrap();
    assert_eq!(out["peerDependenciesMeta"]["react"], json!({ "optional": false }));
    assert_eq!(out["peerDependenciesMeta"]["vue"], json!({ "optional": true }));
}

#[test]
fn string_repository_becomes_git_object() {
    let out = run(json!({
        "name": "foo",
        "version": "1.0.0",
        "repository": "https://github.com/foo/bar",
    }))
    .unwrap();
    assert_eq!(out["repository"], json!({ "type": "git", "url": "https://github.com/foo/bar" }));
}

#[test]
fn object_repository_is_left_untouched() {
    let repo = json!({ "type": "git", "url": "git+ssh://x" });
    let out = run(json!({
        "name": "foo",
        "version": "1.0.0",
        "repository": repo,
    }))
    .unwrap();
    assert_eq!(out["repository"], json!({ "type": "git", "url": "git+ssh://x" }));
}
