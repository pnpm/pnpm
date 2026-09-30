//! Decoding and encoding the JSON documents a registry serves.
//!
//! Every registry document goes through these functions, so they all follow
//! the same rules:
//!
//! - A key given more than once keeps its last value, the way JavaScript's
//!   `JSON.parse` (and so the npm CLI and pnpm v11) reads these documents.
//!   deser rejects repeated keys unless told otherwise.
//! - Floats decode as plain `f64`. The exact decimal text is not kept.
//!
//! Decoding does not limit nesting: see [`pnpm_json_bridge`] for where the
//! depth is capped instead.

pub use pnpm_json_bridge::{MAX_DEPTH, Value, from_serde_json, to_serde_json};

use std::sync::Arc;

use deser::{
    Source, State,
    de::{Deserialize, DeserializeDriver, DuplicateKeys, SinkHandle},
};
use deser_json::{Deserializer, DeserializerConfig};

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
}

#[cfg(test)]
mod tests;
