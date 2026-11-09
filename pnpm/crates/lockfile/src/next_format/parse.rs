//! Recognizing a next-format document, and reading its JSON syntax.

use crate::ComVer;
use serde::de::DeserializeOwned;

/// Which shape a lockfile document's sections have.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// The shape its `lockfileVersion` names.
    OfVersion,
    /// Peer-suffixed, whatever its `lockfileVersion`: the current lockfile,
    /// which records the wanted lockfile's version but is always written
    /// suffixed, so that reading it needs no conversion.
    Suffixed,
}

/// Whether the document is written as JSON, which only the next format is.
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

/// Whether the document is in the next format: written as JSON, or with
/// the next format's `lockfileVersion`. pnpm writes the field on a YAML
/// document's first line, so this reads no further than the first line
/// that sets it.
pub(crate) fn declares_next_format(document: &str) -> bool {
    if is_json_document(document) {
        return true;
    }
    document
        .lines()
        .find_map(|line| line.strip_prefix("lockfileVersion:"))
        .map(|value| value.trim().trim_matches(['\'', '"']))
        .and_then(|value| value.parse::<ComVer>().ok())
        .is_some_and(|version| version.major == crate::NEXT_LOCKFILE_MAJOR)
}
