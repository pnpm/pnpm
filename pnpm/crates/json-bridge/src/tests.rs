use deser_value::Value;
use pretty_assertions::assert_eq;

use super::{MAX_DEPTH, SerdeJson, from_serde_json, to_serde_json};

fn nested_arrays(depth: usize) -> Value {
    deser_json::from_str(&format!("{}{}", "[".repeat(depth), "]".repeat(depth)))
        .expect("decodes at any depth")
}

#[test]
fn nesting_up_to_the_cap_converts() {
    to_serde_json(&nested_arrays(MAX_DEPTH)).expect("MAX_DEPTH levels are allowed");
}

#[test]
fn nesting_past_the_cap_is_decoded_but_not_converted() {
    let deep = nested_arrays(100_000);
    let error = to_serde_json(&deep).expect_err("far past MAX_DEPTH");
    dbg!(&error);
    assert!(error.to_string().contains("recursion limit"));
    to_serde_json(&nested_arrays(MAX_DEPTH + 1)).expect_err("one level past MAX_DEPTH");
}

#[test]
fn to_serde_json_keeps_types_and_key_order() {
    let value: Value =
        deser_json::from_str(r#"{"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}}"#)
            .unwrap();
    let converted = to_serde_json(&value).unwrap();
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
fn yaml_plain_scalars_convert_to_their_values() {
    let value: Value = deser_yaml::from_str(
        "flag: true\ncount: 42\nbelow: -3\nratio: 1.5\nnothing: ~\ntext: plain\nquoted: 'true'\n1.10: key\n",
    )
    .unwrap();
    assert_eq!(
        to_serde_json(&value).unwrap(),
        serde_json::json!({
            "flag": true,
            "count": 42,
            "below": -3,
            "ratio": 1.5,
            "nothing": null,
            "text": "plain",
            "quoted": "true",
            "1.10": "key",
        }),
    );
}

#[test]
fn from_serde_json_is_the_inverse_of_to_serde_json() {
    let original = serde_json::json!({"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}});
    let converted = from_serde_json(&original);
    let expected: Value =
        deser_json::from_str(r#"{"z": 1, "a": [true, null, -2, 1.5, "text"], "m": {"k": {}}}"#)
            .unwrap();
    assert_eq!(converted, expected);
    assert_eq!(to_serde_json(&converted).unwrap(), original);
}

#[derive(Debug, deser::Deserialize)]
struct Holder {
    #[deser(deserialize_as = SerdeJson)]
    tree: serde_json::Value,
    #[deser(deserialize_as = Option<SerdeJson>)]
    optional: Option<serde_json::Value>,
}

#[test]
fn serde_json_adapter_decodes_a_field() {
    let holder: Holder = deser_json::from_str(r#"{"tree": {"a": [1, "b"]}}"#).unwrap();
    dbg!(&holder);
    assert_eq!(holder.tree, serde_json::json!({"a": [1, "b"]}));
    assert_eq!(holder.optional, None);
}

#[test]
fn serde_json_adapter_rejects_nesting_past_the_cap() {
    let deep = format!(r#"{{"tree": {}{}}}"#, "[".repeat(100_000), "]".repeat(100_000));
    let error = deser_json::from_str::<Holder>(&deep).expect_err("far past MAX_DEPTH");
    dbg!(&error);
    assert!(error.to_string().contains("recursion limit"));
}
