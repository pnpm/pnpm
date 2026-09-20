use super::{Error, inline, serialize};
use serde_json::Value;
use std::ops::Range;
use yamlpath::{Document, FeatureKind, Route};

pub(super) type Edit = (Range<usize>, String);

pub(super) fn apply(document: &mut Document, mut edits: Vec<Edit>) -> Result<(), Box<Error>> {
    if edits.is_empty() {
        return Ok(());
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut merged: Vec<Edit> = Vec::new();
    for (range, text) in edits {
        if let Some((last, replacement)) = merged.last_mut()
            && last.end > range.start
            && replacement.is_empty()
            && text.is_empty()
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push((range, text));
        }
    }
    let source = document.source();
    let mut text = String::with_capacity(source.len());
    let mut offset = 0;
    for (range, replacement) in merged {
        if range.start < offset {
            return Err(Box::new(Error::InvalidOperation("Overlapping YAML edits".to_string())));
        }
        text.push_str(&source[offset..range.start]);
        text.push_str(&replacement);
        offset = range.end;
    }
    text.push_str(&source[offset..]);
    *document = Document::new(text).map_err(Error::from)?;
    Ok(())
}

pub(super) fn replace(
    document: &Document,
    edits: &mut Vec<Edit>,
    route: &Route<'_>,
    value: &Value,
) -> Result<(), Box<Error>> {
    let rendered = inline(value)?;
    let feature = document.query_exact(route).map_err(Error::from)?;
    if let Some(feature) = feature {
        let (start, end) = feature.location.byte_span;
        let trailing_newline = document.extract(&feature).ends_with('\n');
        edits.push((start..end, format!("{rendered}{}", if trailing_newline { "\n" } else { "" })));
        Ok(())
    } else {
        let pair = document.query_pretty(route).map_err(Error::from)?;
        let end = pair.location.byte_span.1;
        edits.push((end..end, format!(" {rendered}")));
        Ok(())
    }
}

pub(super) fn append(
    document: &Document,
    edits: &mut Vec<Edit>,
    route: &Route<'_>,
    addition: &Value,
) -> Result<(), Box<Error>> {
    let feature = document
        .query_exact(route)
        .map_err(Error::from)?
        .ok_or_else(|| Error::InvalidOperation("Cannot append to a null YAML value".to_string()))?;
    if matches!(feature.kind(), FeatureKind::FlowMapping | FeatureKind::FlowSequence) {
        let entry = flow_entries(addition)?;
        let (start, end) = feature.location.byte_span;
        append_flow(document, edits, start..end, &entry);
        Ok(())
    } else {
        let content_end = yamlpatch::find_content_end(&feature, document);
        let indent = feature.location.point_span.0.1;
        append_block(document, edits, content_end, indent, addition)
    }
}

fn append_flow(document: &Document, edits: &mut Vec<Edit>, range: Range<usize>, entry: &str) {
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
    edits.push((content_end..content_end, comma.to_string()));
    edits.push((insert_at..insert_at, addition));
}

fn append_block(
    document: &Document,
    edits: &mut Vec<Edit>,
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
    edits.push((offset..offset, text));
    Ok(())
}

fn newline(source: &str) -> &'static str {
    if source.contains("\r\n") { "\r\n" } else { "\n" }
}

fn flow_entries(addition: &Value) -> Result<String, Box<Error>> {
    let entries = match addition {
        Value::Object(mapping) => mapping
            .iter()
            .map(|(key, value)| {
                Ok(format!("{}: {}", inline(&Value::String(key.clone()))?, inline(value)?))
            })
            .collect::<Result<Vec<_>, Box<Error>>>()?,
        Value::Array(values) => values
            .iter()
            .map(inline)
            .collect::<Result<Vec<_>, _>>()?,
        _ => unreachable!("only collections are appended"),
    };
    Ok(entries.join(", "))
}
