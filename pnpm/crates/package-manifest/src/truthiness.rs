use serde_json::Value;

/// Whether a JSON value is truthy under JavaScript's coercion rules, so
/// a Rust guard fires for exactly the values a JS `if (value)` check
/// accepts: `null`, `false`, `0`, and `""` are falsy; every array and
/// object, even an empty one, is truthy.
#[must_use]
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(boolean) => *boolean,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(string) => !string.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}
