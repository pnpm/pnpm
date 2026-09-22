//! Update YAML values while retaining the presentation of unchanged entries.

pub use scalar_aliases::ScalarAliases;
pub use yamlpatch::Error;

mod collection_aliases;
mod edits;
mod patch;
mod scalar_aliases;
mod source_keys;

#[cfg(test)]
mod tests;

use serde_json::Value;
use serde_saphyr::granit_parser::{
    Scanner,
    StrInput,
    Token,
    TokenType,
};
use yamlpath::Document;

/// Decode a manifest, coercing scalar mapping keys to JSON property names.
pub fn parse(text: &str) -> Result<Value, Box<Error>> {
    let value: yaml_serde::Value =
        serde_saphyr::from_str(text).map_err(|error| Error::InvalidOperation(error.to_string()))?;
    json_value(value)
}

fn json_value(value: yaml_serde::Value) -> Result<Value, Box<Error>> {
    Ok(match value {
        yaml_serde::Value::Mapping(mapping) => json_mapping(mapping)?,
        yaml_serde::Value::Sequence(sequence) => Value::Array(
            sequence
                .into_iter()
                .map(json_value)
                .collect::<Result<_, _>>()?,
        ),
        yaml_serde::Value::Tagged(tagged) => json_value(tagged.value)?,
        value => {
            serde_json::to_value(value).map_err(|error| Error::InvalidOperation(error.to_string()))?
        }
    })
}

fn json_mapping(mapping: yaml_serde::Mapping) -> Result<Value, Box<Error>> {
    let mut result = serde_json::Map::new();
    for (key, value) in mapping {
        let key = match json_value(key)? {
            Value::String(key) => key,
            key @ (Value::Null | Value::Bool(_) | Value::Number(_)) => key.to_string(),
            _ => {
                return Err(Box::new(Error::InvalidOperation(
                    "Manifest mapping keys must be scalars".to_string(),
                )));
            }
        };
        if result.contains_key(&key) {
            return Err(Box::new(Error::InvalidOperation(format!(
                "Duplicate manifest key {key:?}",
            ))));
        }
        result.insert(key, json_value(value)?);
    }
    Ok(Value::Object(result))
}

/// Synchronize a YAML document with a JSON-compatible value, preserving existing
/// mapping order and appending new keys.
pub fn sync(text: &str, value: &Value) -> Result<String, Box<Error>> {
    let original = parse(text)?;
    if original == *value {
        return Ok(text.to_string());
    }
    let (text, aliases) = ScalarAliases::expand_changed(text, &original, value)?;
    let insertion = if original.is_null() { empty_document_insertion(&text) } else { None };
    let text = if let Some(offset) = insertion {
        let (before, after) = text.split_at(offset);
        let separator = if before.is_empty() || before.ends_with('\n') { "" } else { "\n" };
        format!("{before}{separator}{}{after}", serialize(value)?)
    } else {
        let mut document = Document::new(text).map_err(Error::from)?;
        collection_aliases::detach_changed(&mut document, &original, value)?;
        patch::sync(&mut document, &original, value)?;
        aliases.restore(document.source())?
    };
    if parse(&text)? != *value {
        return Err(Box::new(Error::InvalidOperation(
            "The YAML edit did not produce the requested manifest".to_string(),
        )));
    }
    Ok(text)
}

fn empty_document_insertion(text: &str) -> Option<usize> {
    let mut insertion = text.len();
    for Token(span, token) in Scanner::new(StrInput::new(text)) {
        match token {
            TokenType::DocumentEnd => insertion = scalar_aliases::byte_range(span).start,
            TokenType::StreamStart(_)
            | TokenType::StreamEnd
            | TokenType::DocumentStart
            | TokenType::VersionDirective(..)
            | TokenType::TagDirective(..)
            | TokenType::Comment(_) => {}
            _ => return None,
        }
    }
    Some(insertion)
}

fn inline(value: &Value) -> Result<String, Box<Error>> {
    if value.is_object()
        || value.is_array()
        || value
            .as_str()
            .is_some_and(|s| s.contains(['\n', '\r', ',', '[', ']', '{', '}']))
    {
        serde_json::to_string(value)
            .map_err(|error| Box::new(Error::InvalidOperation(error.to_string())))
    } else {
        Ok(serialize(value)?.trim_end().to_string())
    }
}

/// Serialize a new YAML document.
pub fn serialize(value: &Value) -> Result<String, Box<Error>> {
    yaml_serde::to_string(value).map_err(|error| Box::new(Error::from(error)))
}
