//! Where the hosted store keeps things, on every backend.
//!
//! ```text
//! <root>/
//!   layout.json                    {"layout": <LAYOUT_VERSION>}
//!   default/                       registries without an `org:`
//!   orgs/<org>/                    one namespace per org
//!   .<record namespace>/           records every namespace shares
//! <namespace>/
//!   packages/<name>/document.json
//!   packages/<name>/blobs/<file>
//!   .<record namespace>/           records of this namespace
//! ```
//!
//! Every name and file name is one [`encode_segment`]ed path segment, so a
//! name can never sit where an org, a namespace, a blob, or another name's
//! child does, and an object key is byte for byte the local path.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The layout this build reads and writes.
pub const LAYOUT_VERSION: u32 = 1;
pub const LAYOUT_FILE: &str = "layout.json";
pub(crate) const PACKAGES_DIR: &str = "packages";
pub(crate) const DOCUMENT_FILE: &str = "document.json";
pub(crate) const BLOBS_DIR: &str = "blobs";
const DEFAULT_NAMESPACE: &str = "default";
const ORGS_DIR: &str = "orgs";
const ESCAPE: u8 = b'!';
const HEX: &[u8; 16] = b"0123456789abcdef";

/// The marker at the root of a hosted store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutMarker {
    pub layout: u32,
}

/// The path segments of an org's namespace, below the store root.
#[must_use]
pub fn namespace_segments(org: &str) -> Vec<String> {
    if org.is_empty() {
        vec![DEFAULT_NAMESPACE.to_string()]
    } else {
        vec![ORGS_DIR.to_string(), encode_segment(org)]
    }
}

/// The directory of package `name` in `org`'s namespace of the local hosted
/// store at `root`.
#[must_use]
pub fn local_package_dir(root: &Path, org: &str, name: &str) -> PathBuf {
    let mut dir = root.to_path_buf();
    dir.extend(namespace_segments(org));
    dir.push(PACKAGES_DIR);
    dir.push(encode_segment(name));
    dir
}

/// Where [`local_package_dir`]'s document is.
#[must_use]
pub fn local_document_path(root: &Path, org: &str, name: &str) -> PathBuf {
    local_package_dir(root, org, name).join(DOCUMENT_FILE)
}

/// Where [`local_package_dir`]'s blob `file` is.
#[must_use]
pub fn local_blob_path(root: &Path, org: &str, name: &str, file: &str) -> PathBuf {
    local_package_dir(root, org, name)
        .join(BLOBS_DIR)
        .join(encode_segment(file))
}

/// Mark the local hosted store at `root` as being in [`LAYOUT_VERSION`].
///
/// # Errors
///
/// Writing the marker failed.
pub fn write_local_layout_marker(root: &Path) -> std::io::Result<()> {
    let marker = serde_json::to_vec(&LayoutMarker { layout: LAYOUT_VERSION })?;
    std::fs::write(root.join(LAYOUT_FILE), marker)
}

/// `text` as one path segment that is the same on a case-insensitive
/// filesystem and in an object key: `[a-z0-9._-]` as is, every other byte as
/// `!` and two lowercase hex digits.
#[must_use]
pub fn encode_segment(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if is_literal(byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push(char::from(ESCAPE));
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0xf)]));
        }
    }
    encoded
}

/// The inverse of [`encode_segment`], or `None` for a segment it cannot have
/// produced.
#[must_use]
pub fn decode_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if byte == ESCAPE {
            decoded.push(decode_escape(bytes.get(idx + 1..idx + 3)?)?);
            idx += 3;
        } else if is_literal(byte) {
            decoded.push(byte);
            idx += 1;
        } else {
            return None;
        }
    }
    String::from_utf8(decoded).ok()
}

/// The byte two lowercase hex digits after an escape stand for, unless
/// [`encode_segment`] would have written that byte as it is.
fn decode_escape(hex: &[u8]) -> Option<u8> {
    if !hex
        .iter()
        .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let byte = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
    (!is_literal(byte)).then_some(byte)
}

/// The bytes [`encode_segment`] keeps as they are.
const fn is_literal(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')
}

#[cfg(test)]
mod tests;
