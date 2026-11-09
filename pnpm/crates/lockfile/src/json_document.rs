//! A lockfile document written as JSON, the next format's syntax: reading
//! it with a trailing comma allowed after the last entry of a collection,
//! and finding its `lockfileVersion` without parsing it.

use serde::de::DeserializeOwned;

/// Whether the document is written as JSON rather than YAML block style.
pub(crate) fn is_json_document(document: &str) -> bool {
    document.trim_start().starts_with('{')
}

/// The document parsed as JSON, with a trailing comma allowed after the last
/// entry of a collection, as the next format writes.
pub(crate) fn from_json_str<Document: DeserializeOwned>(
    document: &str,
) -> Result<Document, serde_json::Error> {
    let mut bytes = document.as_bytes().to_vec();
    blank_trailing_commas(&mut bytes);
    serde_json::from_slice(&bytes)
}

/// Where the scan of a JSON text is.
enum Scan {
    /// Outside a string; `comma` is the last comma, while only whitespace
    /// has followed it.
    Code {
        comma: Option<usize>,
    },
    Text,
    Escape,
}

/// Replaces each comma that only a closing bracket follows with a space,
/// so the offsets, and the line and column an error names, stay the
/// document's.
fn blank_trailing_commas(bytes: &mut [u8]) {
    let mut scan = Scan::Code { comma: None };
    for at in 0..bytes.len() {
        scan = match (scan, bytes[at]) {
            (Scan::Text, b'\\') => Scan::Escape,
            (Scan::Text, b'"') => Scan::Code { comma: None },
            (Scan::Text | Scan::Escape, _) => Scan::Text,
            (Scan::Code { .. }, b'"') => Scan::Text,
            (Scan::Code { .. }, b',') => Scan::Code { comma: Some(at) },
            (Scan::Code { comma }, b'}' | b']') => {
                if let Some(comma) = comma {
                    bytes[comma] = b' ';
                }
                Scan::Code { comma: None }
            }
            (scan @ Scan::Code { .. }, b' ' | b'\n' | b'\r' | b'\t') => scan,
            (Scan::Code { .. }, _) => Scan::Code { comma: None },
        };
    }
}

/// The root object's `lockfileVersion`, read without parsing the document.
/// The scan tracks nesting so that a field of that name inside another
/// object is skipped. `None` when the root has no such string field.
pub(crate) fn json_lockfile_version(document: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut expecting_key = false;
    let mut rest = document;
    loop {
        let index = rest.find(['{', '}', '[', ']', ',', ':', '"'])?;
        let (found, tail) = (rest.as_bytes()[index], &rest[index + 1..]);
        rest = tail;
        match found {
            b'"' => {
                let (text, after) = string_end(tail)?;
                if depth == 1 && expecting_key && text == "lockfileVersion" {
                    return string_value(after);
                }
                rest = after;
                expecting_key = false;
            }
            b'{' | b'[' => {
                depth += 1;
                expecting_key = depth == 1 && found == b'{';
            }
            b'}' | b']' => depth = depth.checked_sub(1)?,
            b',' => expecting_key = depth == 1,
            _ => expecting_key = false,
        }
    }
}

/// The text of a string literal whose opening quote is consumed, and the
/// text after its closing quote.
fn string_end(text: &str) -> Option<(&str, &str)> {
    let mut from = 0;
    loop {
        let index = from + text.get(from..)?.find(['"', '\\'])?;
        if text.as_bytes()[index] == b'"' {
            return Some((&text[..index], &text[index + 1..]));
        }
        from = index + 2;
    }
}

/// The string value after a key, with the `:` between them.
fn string_value(after_key: &str) -> Option<&str> {
    let value = after_key
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    string_end(value).map(|(text, _)| text)
}

#[cfg(test)]
mod tests;
