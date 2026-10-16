//! A settings document as the settings readers see it, so ownership is
//! checked against what they will read.

use serde::de::Error as _;
use yaml_serde::{Mapping, Value};

/// Apply `<<` merge keys, drop tags, and spell every scalar key as a string.
///
/// The settings readers expand merge keys and ignore tags, so a key a merge
/// or a tag hides from a plain reading still sets a setting.
pub(super) fn normalize(mut value: Value) -> Result<Value, yaml_serde::Error> {
    value.apply_merge()?;
    untag(value)
}

/// A `catalogs` value the settings readers accept: a mapping, or nothing.
pub(super) fn check_catalogs(settings: &Mapping) -> Result<(), yaml_serde::Error> {
    match settings.get("catalogs") {
        None | Some(Value::Null | Value::Mapping(_)) => Ok(()),
        Some(_) => Err(yaml_serde::Error::custom(r#""catalogs" must be a mapping of catalogs"#)),
    }
}

fn untag(value: Value) -> Result<Value, yaml_serde::Error> {
    match value {
        Value::Tagged(tagged) => untag(tagged.value),
        Value::Sequence(items) => items
            .into_iter()
            .map(untag)
            .collect::<Result<_, _>>()
            .map(Value::Sequence),
        Value::Mapping(mapping) => untag_mapping(mapping).map(Value::Mapping),
        scalar => Ok(scalar),
    }
}

fn untag_mapping(mapping: Mapping) -> Result<Mapping, yaml_serde::Error> {
    mapping
        .into_iter()
        .map(|(key, value)| Ok((Value::String(key_name(key)?), untag(value)?)))
        .collect()
}

fn key_name(key: Value) -> Result<String, yaml_serde::Error> {
    match key {
        Value::String(name) => Ok(name),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Null => Ok("null".to_string()),
        Value::Tagged(tagged) => key_name(tagged.value),
        Value::Sequence(_) | Value::Mapping(_) => {
            Err(yaml_serde::Error::custom("a mapping key must be a scalar"))
        }
    }
}
