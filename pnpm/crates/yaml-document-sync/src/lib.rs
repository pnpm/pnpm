//! Update YAML values while retaining the presentation of unchanged entries.

pub use scalar_aliases::ScalarAliases;
pub use yamlpatch::Error;

mod collection_aliases;
mod scalar_aliases;

#[cfg(test)]
mod tests;

use serde_json::Value;
use serde_saphyr::granit_parser::{Scanner, StrInput, Token, TokenType};
use std::ops::Range;
use yamlpath::{Document, FeatureKind, Route};

/// Decode a manifest, coercing scalar mapping keys to JSON property names.
pub fn parse(text: &str) -> Result<Value, Box<Error>> {
    let value: yaml_serde::Value =
        serde_saphyr::from_str(text).map_err(|error| Error::InvalidOperation(error.to_string()))?;
    json_value(value)
}

fn json_value(value: yaml_serde::Value) -> Result<Value, Box<Error>> {
    Ok(match value {
        yaml_serde::Value::Mapping(mapping) => {
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
                result.insert(key, json_value(value)?);
            }
            Value::Object(result)
        }
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

/// Synchronize a YAML document with a JSON-compatible value, preserving existing
/// mapping order and appending new keys.
pub fn sync(text: &str, value: &Value) -> Result<String, Box<Error>> {
    let original = parse(text)?;
    if original == *value {
        return Ok(text.to_string());
    }
    let (text, aliases) = ScalarAliases::expand(text)?;
    let insertion = if original.is_null() { empty_document_insertion(&text) } else { None };
    let text = if let Some(offset) = insertion {
        let (before, after) = text.split_at(offset);
        let separator = if before.is_empty() || before.ends_with('\n') { "" } else { "\n" };
        format!("{before}{separator}{}{after}", serialize(value)?)
    } else {
        let mut document = Document::new(text).map_err(Error::from)?;
        collection_aliases::detach_changed(&mut document, &original, value)?;
        patch_value(&mut document, &Route::default(), &original, value)?;
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

fn patch_value(
    document: &mut Document,
    route: &Route<'_>,
    original: &Value,
    value: &Value,
) -> Result<(), Box<Error>> {
    if original == value {
        return Ok(());
    }
    match (original, value) {
        (Value::Object(original), Value::Object(value))
            if !value.is_empty() && !original.is_empty() =>
        {
            patch_mapping(document, route, original, value)?;
        }
        (Value::Array(original), Value::Array(value))
            if !value.is_empty() && !original.is_empty() =>
        {
            patch_sequence(document, route, original, value)?;
        }
        _ => replace(document, route, value)?,
    }
    Ok(())
}

fn patch_sequence(
    document: &mut Document,
    route: &Route<'_>,
    original: &[Value],
    value: &[Value],
) -> Result<(), Box<Error>> {
    for (index, (old, new)) in original.iter().zip(value).enumerate() {
        patch_value(document, &route.with_key(index), old, new)?;
    }
    for index in (value.len()..original.len()).rev() {
        remove(document, &route.with_key(index))?;
    }
    for new in value.iter().skip(original.len()) {
        append(document, route, None, new)?;
    }
    Ok(())
}

fn patch_mapping(
    document: &mut Document,
    route: &Route<'_>,
    original: &serde_json::Map<String, Value>,
    value: &serde_json::Map<String, Value>,
) -> Result<(), Box<Error>> {
    for (key, new) in value {
        if !original.contains_key(key) {
            append(document, route, Some(key), new)?;
        }
    }
    for (key, old) in original {
        let child = route.with_key(key.as_str());
        match value.get(key) {
            Some(new) => patch_value(document, &child, old, new)?,
            None => remove(document, &child)?,
        }
    }
    Ok(())
}

fn splice(
    document: &mut Document,
    range: Range<usize>,
    replacement: &str,
) -> Result<(), Box<Error>> {
    let mut text = document.source().to_string();
    text.replace_range(range, replacement);
    *document = Document::new(text).map_err(Error::from)?;
    Ok(())
}

fn remove(document: &mut Document, route: &Route<'_>) -> Result<(), Box<Error>> {
    let range = document.removal_span(route).map_err(Error::from)?;
    splice(document, range, "")
}

fn replace(document: &mut Document, route: &Route<'_>, value: &Value) -> Result<(), Box<Error>> {
    let rendered = inline(value)?;
    let feature = document.query_exact(route).map_err(Error::from)?;
    if let Some(feature) = feature {
        let (start, end) = feature.location.byte_span;
        let trailing_newline = document.extract(&feature).ends_with('\n');
        splice(
            document,
            start..end,
            &format!("{rendered}{}", if trailing_newline { "\n" } else { "" }),
        )
    } else {
        let pair = document.query_pretty(route).map_err(Error::from)?;
        let end = pair.location.byte_span.1;
        splice(document, end..end, &format!(" {rendered}"))
    }
}

fn append(
    document: &mut Document,
    route: &Route<'_>,
    key: Option<&str>,
    value: &Value,
) -> Result<(), Box<Error>> {
    let feature = document
        .query_exact(route)
        .map_err(Error::from)?
        .ok_or_else(|| Error::InvalidOperation("Cannot append to a null YAML value".to_string()))?;
    if matches!(feature.kind(), FeatureKind::FlowMapping | FeatureKind::FlowSequence) {
        let entry = match key {
            Some(key) => {
                format!("{}: {}", inline(&Value::String(key.to_string()))?, inline(value)?)
            }
            None => inline(value)?,
        };
        let (start, end) = feature.location.byte_span;
        append_flow(document, start..end, &entry)
    } else {
        let content_end = yamlpatch::find_content_end(&feature, document);
        let indent = feature.location.point_span.0.1;
        let addition = match key {
            Some(key) => {
                Value::Object(serde_json::Map::from_iter([(key.to_string(), value.clone())]))
            }
            None => Value::Array(vec![value.clone()]),
        };
        append_block(document, content_end, indent, &addition)
    }
}

fn append_flow(
    document: &mut Document,
    range: Range<usize>,
    entry: &str,
) -> Result<(), Box<Error>> {
    let source = document.source();
    let close = range.end - 1;
    let mut content_end = close;
    while content_end > range.start
        && (source.as_bytes()[content_end - 1].is_ascii_whitespace()
            || document.offset_inside_comment(content_end - 1))
    {
        content_end -= 1;
    }
    let comma = if source.as_bytes()[content_end - 1] == b',' { "" } else { "," };
    let line_start = source[..close]
        .rfind('\n')
        .map_or(close, |index| index + 1);
    let (insert_at, addition) =
        if line_start > range.start && source[line_start..close].trim().is_empty() {
            let indent = source[range]
                .lines()
                .skip(1)
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .map_or("  ", |line| &line[..line.len() - line.trim_start().len()]);
            (line_start, format!("{indent}{entry}{}", newline(source)))
        } else {
            (close, format!(" {entry}"))
        };
    let mut text = source.to_string();
    text.insert_str(insert_at, &addition);
    text.insert_str(content_end, comma);
    *document = Document::new(text).map_err(Error::from)?;
    Ok(())
}

fn append_block(
    document: &mut Document,
    offset: usize,
    indent: usize,
    addition: &Value,
) -> Result<(), Box<Error>> {
    let source = document.source();
    let newline = newline(source);
    let indent = " ".repeat(indent);
    let text = serialize(addition)?
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join(newline);
    let text = if source[..offset].ends_with('\n') {
        format!("{text}{newline}")
    } else {
        format!("{newline}{text}")
    };
    splice(document, offset..offset, &text)
}

fn newline(source: &str) -> &'static str {
    if source.contains("\r\n") { "\r\n" } else { "\n" }
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
