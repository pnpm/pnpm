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

#[test]
fn test_delete_key() {
    let mut value = json!({
        "name": "test-pkg",
        "version": "1.0.0"
    });
    assert!(delete_object_value_by_property_path(&mut value, "version"));
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
    assert!(delete_object_value_by_property_path(&mut value, "scripts.build"));
    assert!(value["scripts"].get("build").is_none());
    assert_eq!(value["scripts"]["test"], "echo test");
}

#[test]
fn test_delete_missing_key() {
    let mut value = json!({
        "name": "test-pkg"
    });
    assert!(!delete_object_value_by_property_path(&mut value, "nonexistent"));
}

#[test]
fn test_delete_array_index() {
    let mut value = json!({
        "files": ["a", "b", "c"]
    });
    assert!(delete_object_value_by_property_path(&mut value, "files[1]"));
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
