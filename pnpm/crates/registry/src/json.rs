//! Decoding and encoding the JSON documents a registry serves.
//!
//! Every registry document goes through these functions, so they all follow
//! the same rules:
//!
//! - A key given more than once keeps its last value, the way JavaScript's
//!   `JSON.parse` (and so the npm CLI and pnpm v11) reads these documents.
//!   deser rejects repeated keys unless told otherwise.
//! - Nesting is capped at [`MAX_DEPTH`] levels, the cap `serde_json`
//!   enforces. deser itself does not recurse, but decoded values are
//!   converted into `serde_json` trees (see [`to_serde_json`]), which are
//!   walked and dropped recursively.
//! - Floats decode as plain `f64`. The exact decimal text is not kept.

use std::sync::Arc;

use deser::{
    Source, State,
    de::{Deserialize, DeserializeDriver, DuplicateKeys, Limits, SinkHandle},
};
use deser_json::{Deserializer, DeserializerConfig};
use deser_value::{Kind, Map, Value};

/// How deeply a registry document may nest.
pub const MAX_DEPTH: usize = 128;

const CONFIG: DeserializerConfig = DeserializerConfig::new().exact_numbers(false);

/// Decode a JSON document. The whole input must be one value.
pub fn from_str<'de, Output: Deserialize<'de>>(json: &'de str) -> Result<Output, deser::Error> {
    let mut deserializer = Deserializer::from_str_with_config(json, &CONFIG);
    let output = deserializer.deserialize_with(configure)?;
    deserializer.end()?;
    Ok(output)
}

/// Decode a JSON document from bytes, validating UTF-8 as it goes.
pub fn from_slice<'de, Output: Deserialize<'de>>(json: &'de [u8]) -> Result<Output, deser::Error> {
    let mut deserializer = Deserializer::from_slice_with_config(json, &CONFIG);
    let output = deserializer.deserialize_with(configure)?;
    deserializer.end()?;
    Ok(output)
}

/// Decode a JSON document whose values may keep references into it.
///
/// The text is published to the deserialization as its [`Source`], which
/// is how [`crate::PackageVersions`] keeps each version as a byte range of
/// the one shared document rather than a copy.
pub fn from_shared_str<Output: for<'de> Deserialize<'de>>(
    json: &Arc<str>,
) -> Result<Output, deser::Error> {
    let mut deserializer = Deserializer::from_str_with_config(json, &CONFIG);
    let output = deserializer.deserialize_with(|driver| {
        configure(driver);
        Source::set(driver.state_mut(), Arc::clone(json));
    })?;
    deserializer.end()?;
    Ok(output)
}

/// Check that `json` is one well-formed JSON value, building nothing.
pub fn validate(json: &str) -> Result<(), deser::Error> {
    from_str::<Ignored>(json).map(|Ignored| ())
}

/// Accepts any JSON value and keeps none of it.
struct Ignored;

impl<'de> Deserialize<'de> for Ignored {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        *out = Some(Ignored);
        SinkHandle::null()
    }
}

/// Encode a value as compact JSON.
pub fn to_string<Input: deser::Serialize>(value: &Input) -> Result<String, deser::Error> {
    deser_json::to_string(value)
}

fn configure(driver: &mut DeserializeDriver<'_, '_>) {
    DuplicateKeys::Last.set(driver.state_mut());
    driver.push_layer(Limits::new().max_depth(MAX_DEPTH));
}

/// Convert a decoded value into the `serde_json` tree the rest of pnpm
/// still passes manifests around as.
///
/// Registry documents only hold JSON's own types. Any other kind of value
/// (bytes, extension values) is converted through its string or number
/// fallback, and becomes `null` without one.
#[must_use]
pub fn to_serde_json(value: &Value) -> serde_json::Value {
    match value.kind() {
        Kind::Null => serde_json::Value::Null,
        Kind::Bool(boolean) => serde_json::Value::Bool(*boolean),
        Kind::U64(number) => serde_json::Value::from(*number),
        Kind::I64(number) => serde_json::Value::from(*number),
        Kind::Seq(items) => items
            .iter()
            .map(to_serde_json)
            .collect(),
        Kind::Map(entries) => entries
            .iter()
            .map(|(key, value)| (key_text(key), to_serde_json(value)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
        _ => scalar_fallback(value),
    }
}

/// Convert a `serde_json` tree into a dynamic [`Value`], the inverse of
/// [`to_serde_json`], for code that still builds manifests with
/// `serde_json`.
#[must_use]
pub fn from_serde_json(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::from(()),
        serde_json::Value::Bool(boolean) => Value::from(*boolean),
        serde_json::Value::Number(number) => number_value(number),
        serde_json::Value::String(text) => Value::from(text.as_str()),
        serde_json::Value::Array(items) => Value::from(
            items
                .iter()
                .map(from_serde_json)
                .collect::<Vec<_>>(),
        ),
        serde_json::Value::Object(entries) => {
            let mut map = Map::with_capacity(entries.len());
            for (key, value) in entries {
                map.insert(key.as_str(), from_serde_json(value));
            }
            Value::from(map)
        }
    }
}

fn number_value(number: &serde_json::Number) -> Value {
    if let Some(unsigned) = number.as_u64() {
        return Value::from(unsigned);
    }
    if let Some(signed) = number.as_i64() {
        return Value::from(signed);
    }
    number
        .as_f64()
        .map_or_else(|| Value::from(()), Value::from)
}

fn scalar_fallback(value: &Value) -> serde_json::Value {
    if let Some(text) = value.as_str() {
        return serde_json::Value::String(text.to_owned());
    }
    value
        .as_f64()
        .and_then(serde_json::Number::from_f64)
        .map_or(serde_json::Value::Null, serde_json::Value::Number)
}

fn key_text(key: &Value) -> String {
    match key.as_str() {
        Some(text) => text.to_owned(),
        None => to_serde_json(key).to_string(),
    }
}

#[cfg(test)]
mod tests;
