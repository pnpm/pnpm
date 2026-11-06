use serde::Deserialize;

/// A defaulted field that also reads YAML null as its default. serde-saphyr
/// rejects null for sequences, mappings, and structs, so a bare `key:` would
/// otherwise fail to load.
pub(super) fn deserialize_null_default<'de, Deserializer, Value>(
    deserializer: Deserializer,
) -> Result<Value, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
    Value: Deserialize<'de> + Default,
{
    Ok(Option::<Value>::deserialize(deserializer)?.unwrap_or_default())
}
