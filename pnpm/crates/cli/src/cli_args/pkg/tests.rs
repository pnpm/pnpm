use super::{
    delete_object_value_by_property_path, fix_manifest, get_output,
    set_object_value_by_property_path,
};
use serde_json::{Value, json};

#[test]
fn test_get_single_key() {
    let manifest = json!({
        "name": "test-pkg",
        "version": "1.0.0",
        "scripts": {
            "test": "echo test"
        }
    });
    let result = get_output(&manifest, &["version".to_string()], false).unwrap();
    assert_eq!(result, "1.0.0");
}

#[test]
fn test_get_single_key_json() {
    let manifest = json!({
        "name": "test-pkg",
        "description": "hello",
    });
    let result = get_output(&manifest, &["description".to_string()], true).unwrap();
    assert_eq!(result, r#""hello""#);
}

#[test]
fn test_get_single_key_nested() {
    let manifest = json!({
        "scripts": {
            "test": "echo test"
        }
    });
    let result = get_output(&manifest, &["scripts.test".to_string()], false).unwrap();
    assert_eq!(result, "echo test");
}

#[test]
fn test_get_hyphenated_key_nested() {
    let manifest = json!({
        "dependencies": {
            "some-package-name": "0.23.4"
        }
    });
    let result =
        get_output(&manifest, &["dependencies.some-package-name".to_string()], false).unwrap();
    assert_eq!(result, "0.23.4");
}

#[test]
fn test_get_single_key_object() {
    let manifest = json!({
        "scripts": {
            "test": "echo test"
        }
    });
    let result = get_output(&manifest, &["scripts".to_string()], false).unwrap();
    assert_eq!(result, "{\n  \"test\": \"echo test\"\n}");
}

#[test]
fn test_get_missing_key() {
    let manifest = json!({
        "name": "test-pkg"
    });
    let result = get_output(&manifest, &["nonexistent".to_string()], false).unwrap();
    assert_eq!(result, "");
}

#[test]
fn test_get_multiple_keys() {
    let manifest = json!({
        "name": "test-pkg",
        "version": "1.0.0",
        "description": "a test"
    });
    let result =
        get_output(&manifest, &["name".to_string(), "version".to_string()], false).unwrap();
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["name"], "test-pkg");
    assert_eq!(parsed["version"], "1.0.0");
    assert!(parsed.get("description").is_none());
}

#[test]
fn test_get_no_keys_returns_full_manifest() {
    let manifest = json!({
        "name": "test-pkg",
        "version": "1.0.0"
    });
    let result = get_output(&manifest, &[] as &[String], false).unwrap();
    assert_eq!(result, "{\n  \"name\": \"test-pkg\",\n  \"version\": \"1.0.0\"\n}");
}

#[test]
fn test_get_multiple_keys_returns_json_object() {
    let manifest = json!({
        "name": "test-pkg",
        "version": "1.0.0"
    });
    let keys: Vec<String> = vec!["name".to_string(), "version".to_string()];
    let result = get_output(&manifest, &keys, false).unwrap();
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["name"], "test-pkg");
    assert_eq!(parsed["version"], "1.0.0");
}

#[test]
fn test_fix_removes_invalid_name() {
    let mut manifest = json!({
        "name": 123,
        "version": "1.0.0"
    });
    fix_manifest(&mut manifest);
    assert!(manifest.get("name").is_none());
    assert_eq!(manifest["version"], "1.0.0");
}

#[test]
fn test_fix_keeps_valid_name() {
    let mut manifest = json!({
        "name": "valid-pkg",
        "version": "1.0.0"
    });
    fix_manifest(&mut manifest);
    assert_eq!(manifest["name"], "valid-pkg");
}

#[test]
fn test_fix_removes_invalid_dependencies() {
    let mut manifest = json!({
        "dependencies": "not-an-object"
    });
    fix_manifest(&mut manifest);
    assert!(manifest.get("dependencies").is_none());
}

#[test]
fn test_fix_removes_array_bin() {
    let mut manifest = json!({
        "bin": ["should", "be", "string", "or", "object"]
    });
    fix_manifest(&mut manifest);
    assert!(manifest.get("bin").is_none());
}

#[test]
fn test_fix_keeps_string_bin() {
    let mut manifest = json!({
        "bin": "./bin.js"
    });
    fix_manifest(&mut manifest);
    assert_eq!(manifest["bin"], "./bin.js");
}

#[test]
fn test_fix_keeps_object_bin() {
    let mut manifest = json!({
        "bin": {"myapp": "./bin.js"}
    });
    fix_manifest(&mut manifest);
    assert_eq!(manifest["bin"]["myapp"], "./bin.js");
}

fn fix_repository(repository: Value) -> Value {
    let mut manifest = json!({ "name": "test-pkg" });
    manifest["repository"] = repository;
    fix_manifest(&mut manifest);
    manifest["repository"].take()
}

#[test]
fn test_fix_converts_repository_string_to_object() {
    for (repository, url) in [
        ("github:pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("https://github.com/pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("https://github.com/pnpm/example.git", "git+https://github.com/pnpm/example.git"),
        ("https://www.github.com/pnpm/example", "git+https://github.com/pnpm/example.git"),
        ("git+https://github.com/pnpm/example.git", "git+https://github.com/pnpm/example.git"),
        ("git@github.com:pnpm/example.git", "git+ssh://git@github.com/pnpm/example.git"),
        ("ssh://git@github.com/pnpm/example.git", "git+ssh://git@github.com/pnpm/example.git"),
        ("git+ssh://git@github.com/pnpm/example.git", "git+ssh://git@github.com/pnpm/example.git"),
        ("http://github.com/pnpm/example", "git+ssh://git@github.com/pnpm/example.git"),
        ("git://github.com/pnpm/example.git", "git://github.com/pnpm/example.git"),
        ("gitlab:foo/bar", "git+https://gitlab.com/foo/bar.git"),
        ("https://gitlab.com/group/sub/proj", "git+https://gitlab.com/group/sub/proj.git"),
        ("git://gitlab.com/foo/bar.git", "git://gitlab.com/foo/bar.git"),
        ("bitbucket:foo/bar", "git+https://bitbucket.org/foo/bar.git"),
        ("github:pnpm/example#v1", "git+https://github.com/pnpm/example.git#v1"),
        ("github:pnpm/example#semver:^1", "git+https://github.com/pnpm/example.git#semver:^1"),
        ("https://example.com/repo.git", "https://example.com/repo.git"),
    ] {
        let fixed = fix_repository(json!(repository));
        assert_eq!(fixed, json!({ "type": "git", "url": url }), "repository: {repository}");
        let keys: Vec<&String> = fixed
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(keys, ["type", "url"], "repository: {repository}");
    }
}

#[test]
fn test_fix_normalizes_gist_repository_url() {
    assert_eq!(
        fix_repository(json!("gist:abc123")),
        json!({ "type": "git", "url": "git+https://gist.github.com/abc123.git" }),
    );
}

#[test]
fn test_fix_keeps_ownerless_repository_shortcut() {
    assert_eq!(
        fix_repository(json!("github:skills")),
        json!({ "type": "git", "url": "github:skills" }),
    );
}

#[test]
fn test_fix_normalizes_repository_object_url() {
    assert_eq!(
        fix_repository(json!({
            "type": "git",
            "url": "github:pnpm/example",
            "directory": "packages/x",
        })),
        json!({
            "type": "git",
            "url": "git+https://github.com/pnpm/example.git",
            "directory": "packages/x",
        }),
    );
}

#[test]
fn test_fix_does_not_add_repository_type() {
    assert_eq!(
        fix_repository(json!({ "url": "https://github.com/pnpm/example" })),
        json!({ "url": "git+https://github.com/pnpm/example.git" }),
    );
}

#[test]
fn test_fix_keeps_unfixable_repository() {
    for repository in [
        json!(""),
        json!({ "type": "git", "url": 42 }),
        json!(["github:pnpm/example"]),
        json!(true),
    ] {
        assert_eq!(fix_repository(repository.clone()), repository);
    }
}

#[test]
fn test_delete_key() {
    let mut value = json!({
        "name": "test-pkg",
        "version": "1.0.0"
    });
    assert!(delete_object_value_by_property_path(&mut value, "version").unwrap());
    assert!(value.get("version").is_none());
    assert_eq!(value["name"], "test-pkg");
}

#[test]
fn test_delete_nested_key() {
    let mut value = json!({
        "scripts": {
            "test": "echo test",
            "build": "tsc"
        }
    });
    assert!(delete_object_value_by_property_path(&mut value, "scripts.build").unwrap());
    assert!(value["scripts"].get("build").is_none());
    assert_eq!(value["scripts"]["test"], "echo test");
}

#[test]
fn test_delete_missing_key() {
    let mut value = json!({
        "name": "test-pkg"
    });
    assert!(!delete_object_value_by_property_path(&mut value, "nonexistent").unwrap());
}

#[test]
fn test_delete_array_index() {
    let mut value = json!({
        "files": ["a", "b", "c"]
    });
    assert!(delete_object_value_by_property_path(&mut value, "files[1]").unwrap());
    assert_eq!(value["files"], json!(["a", "c"]));
}

#[test]
fn test_set_key_value() {
    let mut value = json!({
        "name": "old"
    });
    set_object_value_by_property_path(&mut value, "name", json!("new")).unwrap();
    assert_eq!(value["name"], "new");
}

#[test]
fn test_set_new_key() {
    let mut value = json!({
        "name": "test-pkg"
    });
    set_object_value_by_property_path(&mut value, "version", json!("2.0.0")).unwrap();
    assert_eq!(value["version"], "2.0.0");
}

#[test]
fn test_set_nested_key() {
    let mut value = json!({
        "scripts": {
            "test": "echo test"
        }
    });
    set_object_value_by_property_path(&mut value, "scripts.build", json!("tsc")).unwrap();
    assert_eq!(value["scripts"]["build"], "tsc");
}

#[test]
fn test_set_hyphenated_key_nested() {
    let mut value = json!({
        "name": "test-pkg"
    });
    set_object_value_by_property_path(
        &mut value,
        "dependencies.some-package-name",
        json!("0.23.4"),
    )
    .unwrap();
    assert_eq!(value["dependencies"], json!({ "some-package-name": "0.23.4" }));
}

#[test]
fn test_set_creates_intermediate_objects() {
    let mut value = json!({
        "name": "test-pkg"
    });
    set_object_value_by_property_path(&mut value, "scripts.test", json!("jest")).unwrap();
    assert_eq!(value["scripts"]["test"], "jest");
}

#[test]
fn test_set_array_index() {
    let mut value = json!({
        "files": ["a", "b"]
    });
    set_object_value_by_property_path(&mut value, "files[0]", json!("x")).unwrap();
    assert_eq!(value["files"][0], "x");
}

#[test]
fn test_set_array_new_index() {
    let mut value = json!({
        "files": ["a"]
    });
    set_object_value_by_property_path(&mut value, "files[2]", json!("c")).unwrap();
    assert_eq!(value["files"], json!(["a", null, "c"]));
}

#[test]
fn test_set_rejects_unsafe_key() {
    let mut value = json!({ "name": "test-pkg" });
    let result = set_object_value_by_property_path(&mut value, "__proto__.polluted", json!(true));
    assert!(result.is_err());
    assert!(value.get("__proto__").is_none());
}

#[test]
fn test_set_rejects_constructor_key() {
    let mut value = json!({ "name": "test-pkg" });
    let result = set_object_value_by_property_path(
        &mut value,
        "constructor.prototype.polluted",
        json!(true),
    );
    assert!(result.is_err());
}

#[test]
fn test_set_creates_array_from_scratch() {
    let mut value = json!({});
    set_object_value_by_property_path(&mut value, "keywords[0]", json!("cli")).unwrap();
    assert_eq!(value["keywords"], json!(["cli"]));
}

#[test]
fn test_set_creates_array_intermediate() {
    let mut value = json!({});
    set_object_value_by_property_path(&mut value, "keywords[0].name", json!("cli")).unwrap();
    assert_eq!(value["keywords"][0]["name"], "cli");
}

#[test]
fn test_set_replaces_scalar_with_object() {
    let mut value = json!({ "foo": "bar" });
    set_object_value_by_property_path(&mut value, "foo.baz", json!("qux")).unwrap();
    assert_eq!(value["foo"]["baz"], "qux");
}

#[test]
fn test_set_replaces_scalar_with_array() {
    let mut value = json!({ "foo": "bar" });
    set_object_value_by_property_path(&mut value, "foo[0]", json!("qux")).unwrap();
    assert_eq!(value["foo"][0], "qux");
}

#[test]
fn test_set_rejects_empty_path() {
    let mut value = json!({ "name": "test-pkg" });
    let result = set_object_value_by_property_path(&mut value, "", json!("value"));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("empty property path"));
}

#[test]
fn test_set_rejects_too_large_index() {
    let mut value = json!({});
    let big_idx = format!("x[{}]", super::editing::MAX_ARRAY_INDEX + 1);
    let result = set_object_value_by_property_path(&mut value, &big_idx, json!("value"));
    assert!(result.is_err());
}

#[test]
fn test_delete_rejects_unsafe_key() {
    let mut value = json!({ "name": "test-pkg" });
    let result = delete_object_value_by_property_path(&mut value, "__proto__.polluted");
    assert!(result.is_err());
}

#[test]
fn test_set_numeric_key_on_object() {
    let mut value = json!({ "0": "existing" });
    set_object_value_by_property_path(&mut value, "[0]", json!("replaced")).unwrap();
    assert_eq!(value["0"], "replaced");
}

#[test]
fn test_get_output_rejects_empty_key() {
    let manifest = json!({ "name": "test-pkg" });
    let result = get_output(&manifest, &[String::new()], false);
    assert!(result.is_err());
}

#[test]
fn test_delete_with_numeric_key_on_object() {
    let mut value = json!({ "123": "number-key" });
    assert!(delete_object_value_by_property_path(&mut value, "[123]").unwrap());
    assert!(value.get("123").is_none());
}
