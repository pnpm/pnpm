use super::{reject_unsafe_key, set_script};
use serde_json::json;

#[test]
fn creates_scripts_object_when_absent() {
    let mut manifest = json!({ "name": "x" });
    set_script(&mut manifest, "build".to_string(), "tsc".to_string());
    assert_eq!(manifest["scripts"], json!({ "build": "tsc" }));
}

#[test]
fn replaces_a_non_object_scripts_field() {
    let mut manifest = json!({ "scripts": "oops" });
    set_script(&mut manifest, "build".to_string(), "tsc".to_string());
    assert_eq!(manifest["scripts"], json!({ "build": "tsc" }));
}

#[test]
fn uses_a_dotted_name_as_a_literal_key() {
    let mut manifest = json!({});
    set_script(&mut manifest, "pre.publish".to_string(), "echo".to_string());
    assert_eq!(manifest["scripts"], json!({ "pre.publish": "echo" }));
}

#[test]
fn reject_unsafe_key_blocks_prototype_keys() {
    for key in ["__proto__", "constructor", "prototype"] {
        eprintln!("rejecting unsafe key: {key}");
        assert!(reject_unsafe_key(key).is_err());
    }
    assert!(reject_unsafe_key("build").is_ok());
}
