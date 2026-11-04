//! The canonical JSON TUF signs, as `securesystemslib` writes it: object
//! keys sorted, no insignificant whitespace, integers only, and only `"` and
//! `\` escaped in strings.

use super::TufError;
use serde_json::{Map, Number, Value};

/// The canonical encoding of `value`.
pub(crate) fn encode(value: &Value) -> Result<Vec<u8>, TufError> {
    let mut encoded = Vec::new();
    write_value(value, &mut encoded)?;
    Ok(encoded)
}

fn write_value(value: &Value, encoded: &mut Vec<u8>) -> Result<(), TufError> {
    match value {
        Value::Null => encoded.extend_from_slice(b"null"),
        Value::Bool(flag) => encoded.extend_from_slice(if *flag { b"true" } else { b"false" }),
        Value::Number(number) => write_integer(number, encoded)?,
        Value::String(text) => write_string(text, encoded),
        Value::Array(items) => write_array(items, encoded)?,
        Value::Object(object) => write_object(object, encoded)?,
    }
    Ok(())
}

fn write_integer(number: &Number, encoded: &mut Vec<u8>) -> Result<(), TufError> {
    let integer = number.as_i64().map(|signed| signed.to_string());
    let integer = integer.or_else(|| number.as_u64().map(|unsigned| unsigned.to_string()));
    encoded.extend_from_slice(integer.ok_or(TufError::NonIntegerNumber)?.as_bytes());
    Ok(())
}

fn write_array(items: &[Value], encoded: &mut Vec<u8>) -> Result<(), TufError> {
    encoded.push(b'[');
    for (position, item) in items.iter().enumerate() {
        if position > 0 {
            encoded.push(b',');
        }
        write_value(item, encoded)?;
    }
    encoded.push(b']');
    Ok(())
}

fn write_object(object: &Map<String, Value>, encoded: &mut Vec<u8>) -> Result<(), TufError> {
    let mut entries: Vec<_> = object.iter().collect();
    entries.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    encoded.push(b'{');
    for (position, (key, item)) in entries.into_iter().enumerate() {
        if position > 0 {
            encoded.push(b',');
        }
        write_string(key, encoded);
        encoded.push(b':');
        write_value(item, encoded)?;
    }
    encoded.push(b'}');
    Ok(())
}

fn write_string(text: &str, encoded: &mut Vec<u8>) {
    encoded.push(b'"');
    for byte in text.bytes() {
        if matches!(byte, b'"' | b'\\') {
            encoded.push(b'\\');
        }
        encoded.push(byte);
    }
    encoded.push(b'"');
}
