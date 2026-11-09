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

/// The string after the first `"lockfileVersion":` of a JSON document.
pub(crate) fn json_lockfile_version(document: &str) -> Option<&str> {
    const KEY: &str = r#""lockfileVersion""#;
    let after_key = &document[document.find(KEY)? + KEY.len()..];
    let value = after_key
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let value = value.strip_prefix('"')?;
    value.split('"').next()
}
