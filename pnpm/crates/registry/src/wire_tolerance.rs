//! Tolerant decoding for registry wire-format variance.
//!
//! The npm wire format is a de-facto contract rather than a specified
//! one, and registries mirroring npm diverge on the shape of individual
//! fields. That divergence is unusually expensive here: a version
//! manifest that fails to decode is skipped as though the registry never
//! published the version, so a single unmodeled field can erase
//! `dist-tags.latest` from a packument and strand resolution.
//!
//! Fields whose value pnpm does not depend on are therefore decoded
//! leniently — the field degrades to `None`, never the version. Fields
//! pnpm *does* depend on stay strict, because a version that cannot be
//! installed safely must fail loudly rather than quietly: see
//! [`PackageDistribution::integrity`](crate::PackageDistribution), where
//! an unusable value has to reach the install as an error.

use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

/// Deserialize a field pnpm reads for presence alone, keeping the typed
/// body when the registry sends one and decoding any other shape as
/// present-with-no-detail.
///
/// npm serves these markers as objects, but their body is not part of
/// the wire contract that registries mirroring npm honor — some
/// abbreviate a marker to a bare flag such as `1`. Nothing downstream
/// reads the body, only whether the marker is there, so an unrecognized
/// one must not cost the version.
pub(crate) fn deserialize_presence_marker<'de, Marker, Deser>(
    deserializer: Deser,
) -> Result<Option<Marker>, Deser::Error>
where
    Marker: DeserializeOwned + Default,
    Deser: Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_value(value).unwrap_or_default()))
}

/// Deserialize a record whose fields pnpm reads, tolerating a registry
/// that sends something other than an object in its place.
///
/// Unlike [`deserialize_presence_marker`], the container's *presence*
/// carries no signal of its own — everything pnpm wants is inside it —
/// so a non-object decodes as absent rather than as an empty record.
pub(crate) fn deserialize_record_or_absent<'de, Record, Deser>(
    deserializer: Deser,
) -> Result<Option<Record>, Deser::Error>
where
    Record: DeserializeOwned,
    Deser: Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if !value.is_object() {
        return Ok(None);
    }
    Ok(serde_json::from_value(value).ok())
}

/// Deserialize a byte/entry count the resolver treats as advisory,
/// accepting the numeric shapes a registry's serializer may emit.
///
/// A JSON number that is integral and non-negative decodes whatever its
/// encoding — `12345` and `12345.0` are the same count, and a registry
/// whose backend round-trips through a float (Go's `float64`, Python's
/// `json`, Ruby's `JSON`) emits the latter. A numeric string is accepted
/// on the same reasoning. Anything else decodes as absent, which is
/// already a shape every reader handles.
pub(crate) fn deserialize_advisory_count<'de, Deser>(
    deserializer: Deser,
) -> Result<Option<usize>, Deser::Error>
where
    Deser: Deserializer<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    Ok(match value {
        Value::Number(number) => integral_count(&number),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    })
}

fn integral_count(number: &serde_json::Number) -> Option<usize> {
    if let Some(exact) = number.as_u64() {
        return usize::try_from(exact).ok();
    }
    let float = number.as_f64()?;
    (float.is_finite() && float >= 0.0 && float.fract() == 0.0).then_some(float as usize)
}

/// Deserialize a flag that only counts when the registry sends a real
/// boolean.
///
/// The TypeScript resolver compares these with `=== true`, so a string
/// `"true"` is not a `true` there and must not become one here. Decoding
/// every non-boolean as absent reproduces that comparison exactly while
/// keeping an off-shape value from costing the whole version.
pub(crate) fn deserialize_strict_flag<'de, Deser>(
    deserializer: Deser,
) -> Result<Option<bool>, Deser::Error>
where
    Deser: Deserializer<'de>,
{
    Ok(Option::<Value>::deserialize(deserializer)?.and_then(|value| value.as_bool()))
}

#[cfg(test)]
mod tests;
