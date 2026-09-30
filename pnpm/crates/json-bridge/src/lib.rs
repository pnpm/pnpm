//! Conversion between deser's dynamic [`Value`]s and `serde_json` trees,
//! for the code that decodes documents with deser but still passes values
//! around as `serde_json::Value`.
//!
//! deser keeps nested values on the heap rather than the call stack, and
//! [`Value`]s are compared, cloned, and dropped without recursion, so a
//! document can be decoded whatever its depth. The conversion into a
//! `serde_json` tree is recursive, and so is everything that later walks
//! or drops the tree. [`to_serde_json`] therefore enforces `serde_json`'s
//! own cap of [`MAX_DEPTH`] levels. Capping the depth while decoding
//! (deser's `Limits` layer) would cost time on every event of every
//! document instead.

pub use deser_value::Value;

use deser::{
    ErrorKind, ImplicitValue, State,
    adapters::{DeserializeAs, FromInto, TryFromInto},
    de::{Deserialize, SinkHandle},
};
use deser_value::{Kind, Map};

/// How deeply a value converted into a `serde_json` tree may nest, the
/// limit `serde_json` enforces when it parses.
pub const MAX_DEPTH: usize = 128;

/// Convert a decoded value into a `serde_json` tree.
///
/// JSON's own types convert as they are, and so do the YAML scalars a
/// format resolves to null, booleans, and numbers. Any other kind of value
/// (bytes, extension values) is converted through its string or number
/// fallback, and becomes `null` without one. A value nested deeper than
/// [`MAX_DEPTH`] levels is rejected, like `serde_json` rejects the
/// document it would come from.
pub fn to_serde_json(value: &Value) -> Result<serde_json::Value, deser::Error> {
    convert(value, MAX_DEPTH)
}

/// `levels_left` is how many more containers may be entered, the one
/// `value` opens included.
fn convert(value: &Value, levels_left: usize) -> Result<serde_json::Value, deser::Error> {
    let inner_levels = || levels_left.checked_sub(1).ok_or_else(too_deep);
    match value.kind() {
        Kind::Null => Ok(serde_json::Value::Null),
        Kind::Bool(boolean) => Ok(serde_json::Value::Bool(*boolean)),
        Kind::U64(number) => Ok(serde_json::Value::from(*number)),
        Kind::I64(number) => Ok(serde_json::Value::from(*number)),
        Kind::Implicit(implicit) => Ok(implicit_value(implicit.value())),
        Kind::Seq(items) => {
            let levels_left = inner_levels()?;
            items
                .iter()
                .map(|item| convert(item, levels_left))
                .collect()
        }
        Kind::Map(entries) => {
            let levels_left = inner_levels()?;
            entries
                .iter()
                .map(|(key, value)| Ok((key_text(key), convert(value, levels_left)?)))
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(serde_json::Value::Object)
        }
        _ => Ok(scalar_fallback(value)),
    }
}

fn too_deep() -> deser::Error {
    deser::Error::new(ErrorKind::Unexpected, "recursion limit exceeded")
}

fn implicit_value(value: ImplicitValue) -> serde_json::Value {
    match value {
        ImplicitValue::Bool(boolean) => serde_json::Value::Bool(boolean),
        ImplicitValue::U64(number) => serde_json::Value::from(number),
        ImplicitValue::I64(number) => serde_json::Value::from(number),
        ImplicitValue::F64(number) => float_value(number),
        _ => serde_json::Value::Null,
    }
}

fn scalar_fallback(value: &Value) -> serde_json::Value {
    if let Some(text) = value.as_str() {
        return serde_json::Value::String(text.to_owned());
    }
    value.as_f64().map_or(serde_json::Value::Null, float_value)
}

fn float_value(number: f64) -> serde_json::Value {
    serde_json::Number::from_f64(number).map_or(serde_json::Value::Null, serde_json::Value::Number)
}

/// A `serde_json` key is the text of the key as written, so a YAML key
/// such as `1.10` stays `"1.10"` rather than becoming the number's text.
fn key_text(key: &Value) -> String {
    if let Some(text) = key.as_str() {
        return text.to_owned();
    }
    if let Kind::Implicit(implicit) = key.kind() {
        return implicit.text().to_string();
    }
    scalar_fallback(key).to_string()
}

/// Convert a `serde_json` tree into a dynamic [`Value`], the inverse of
/// [`to_serde_json`], for code that still builds values with `serde_json`.
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

/// Decodes a `serde_json::Value` field, for use as
/// `#[deser(deserialize_as = SerdeJson)]` (or `Option<SerdeJson>`, or
/// inside [`As`](deser::adapters::As)).
///
/// The value is decoded into a [`Value`] and converted with
/// [`to_serde_json`], so it is held to [`MAX_DEPTH`]. deser-serde's
/// `Serde` adapter would instead build the tree with serde's recursive
/// visitor, which has no depth cap of its own.
pub struct SerdeJson;

impl<'de> DeserializeAs<'de, serde_json::Value> for SerdeJson {
    fn deserialize_into_as<'out>(
        out: &'out mut Option<serde_json::Value>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <FromInto<ConvertedTree> as DeserializeAs<'de, serde_json::Value>>::deserialize_into_as(
            out, state,
        )
    }
}

/// A `serde_json` tree converted from a decoded [`Value`], the step
/// between the two that [`SerdeJson`] goes through.
struct ConvertedTree(serde_json::Value);

impl<'de> Deserialize<'de> for ConvertedTree {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <TryFromInto<Value> as DeserializeAs<'de, Self>>::deserialize_into_as(out, state)
    }
}

/// The error is the message alone: the adapter reports it as an invalid
/// value, which already says what kind of error it is.
impl TryFrom<Value> for ConvertedTree {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        to_serde_json(&value)
            .map(ConvertedTree)
            .map_err(|error| error.message().to_owned())
    }
}

impl From<ConvertedTree> for serde_json::Value {
    fn from(tree: ConvertedTree) -> Self {
        tree.0
    }
}

#[cfg(test)]
mod tests;
