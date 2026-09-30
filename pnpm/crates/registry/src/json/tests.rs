use std::collections::HashMap;

use deser_value::Value;
use pretty_assertions::assert_eq;

use super::{from_slice, from_str, validate};

#[test]
fn a_repeated_key_keeps_its_last_value() {
    let map: HashMap<String, String> = from_str(r#"{"a": "first", "a": "last"}"#).unwrap();
    assert_eq!(map["a"], "last");
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
