use std::collections::HashMap;

use deser_value::Value;
use pretty_assertions::assert_eq;

use super::{MAX_DEPTH, from_serde_json, from_slice, from_str, to_serde_json, validate};

#[test]
fn a_repeated_key_keeps_its_last_value() {
    let map: HashMap<String, String> = from_str(r#"{"a": "first", "a": "last"}"#).unwrap();
    assert_eq!(map["a"], "last");
}

#[test]
fn nesting_up_to_the_cap_decodes() {
    let json = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    from_str::<Value>(&json).expect("MAX_DEPTH levels are allowed");
}

#[test]
fn nesting_past_the_cap_is_rejected() {
    let json = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    let error = from_str::<Value>(&json).expect_err("one level past MAX_DEPTH");
    dbg!(&error);
    assert!(error.to_string().contains("recursion limit"));
}

#[test]
fn trailing_content_is_rejected() {
    from_str::<Value>("{} {}").expect_err("two values");
    from_slice::<Value>(b"[] x").expect_err("garbage after the value");
}

#[test]
fn validate_accepts_one_value_and_rejects_anything_else() {
    validate(r#"{"a": [1, {"b": null}]}"#).expect("a well-formed value");
    validate(r#"{"a": }"#).expect_err("malformed");
    validate("").expect_err("no value");
}

#[test]
fn to_serde_json_keeps_types_and_key_order() {
    let value: Value =
        from_str(r#"{"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}}"#).unwrap();
    let converted = to_serde_json(&value);
    assert_eq!(
        converted,
        serde_json::json!({"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}}),
    );
    let keys: Vec<&str> = converted
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["z", "a", "m"]);
}

#[test]
fn from_serde_json_is_the_inverse_of_to_serde_json() {
    let original = serde_json::json!({"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}});
    let converted = from_serde_json(&original);
    let expected: Value =
        from_str(r#"{"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}}"#).unwrap();
    assert_eq!(converted, expected);
    assert_eq!(to_serde_json(&converted), original);
}
