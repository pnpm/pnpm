//! Where the hosted store keeps things, on every backend.
//!
//! ```text
//! <root>/
//!   layout.json                    {"layout": <LAYOUT_VERSION>}
//!   default/<ecosystem>/           registries without an `org:`
//!   orgs/<org>/<ecosystem>/        one namespace per (org, ecosystem)
//!   .<record namespace>/           records every namespace shares
//! <namespace>/
//!   archives/<shard>/<identity>.<incarnation>
//!   .<record namespace>/           records of this namespace
//! <cache root>/<upstream>/
//!   packages/<segments...>/document.json
//!   packages/<segments...>/blobs/<file>
//! ```
//!
//! A hosted archive is stored by the identity of its bytes (see
//! [`crate::BlobId`]), under the two first hex digits of its digest, so the
//! bytes several files share are stored once and a public file name never
//! reaches a key. The proxy cache keeps an upstream's files by name: a
//! package is the segments of [`package_segments`] and a blob the one segment
//! of [`blob_segment`], so a name can never sit where an org, a namespace, a
//! blob, or another name's child does, and an object key is byte for byte the
//! local path.

use pnpr_package_name::Ecosystem;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The layout this build reads and writes.
pub const LAYOUT_VERSION: u32 = 2;
pub const LAYOUT_FILE: &str = "layout.json";
pub(crate) const PACKAGES_DIR: &str = "packages";
pub(crate) const DOCUMENT_FILE: &str = "document.json";
pub(crate) const BLOBS_DIR: &str = "blobs";
/// Where a hosted namespace keeps its archives, by key.
pub(crate) const ARCHIVES_DIR: &str = "archives";
const DEFAULT_NAMESPACE: &str = "default";
const ORGS_DIR: &str = "orgs";
/// Opens every non-leaf segment of [`package_segments`], and the segment of a
/// [`VERSIONED_EXTENSIONS`] file not named after its package.
/// [`encode_segment`] never writes it.
pub(crate) const MARKER: char = '@';
/// The files named `<basename>-<version><extension>`, which [`blob_segment`]
/// stores without the `<basename>-`. Adding an extension changes how a stored
/// file with it decodes, so one joins only with the ecosystem that introduces
/// it, and only when no other ecosystem stores files with it.
const VERSIONED_EXTENSIONS: [&str; 3] = [".tgz", ".sigstore", ".crate"];
const ESCAPE: u8 = b'!';
const HEX: &[u8; 16] = b"0123456789abcdef";

/// The marker at the root of a hosted store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutMarker {
    pub layout: u32,
}

/// The path segments of an `(org, ecosystem)` namespace, below the store root.
#[must_use]
pub fn namespace_segments(org: &str, ecosystem: Ecosystem) -> Vec<String> {
    if org.is_empty() {
        vec![DEFAULT_NAMESPACE.to_string(), ecosystem.as_str().to_string()]
    } else {
        vec![ORGS_DIR.to_string(), encode_segment(org), ecosystem.as_str().to_string()]
    }
}

/// The directory of package `name` in `(org, ecosystem)`'s namespace of the
/// local hosted store at `root`.
#[must_use]
pub fn local_package_dir(root: &Path, org: &str, ecosystem: Ecosystem, name: &str) -> PathBuf {
    let mut dir = root.to_path_buf();
    dir.extend(namespace_segments(org, ecosystem));
    dir.push(PACKAGES_DIR);
    dir.extend(package_segments(ecosystem, name));
    dir
}

/// Where [`local_package_dir`]'s document is.
#[must_use]
pub fn local_document_path(root: &Path, org: &str, ecosystem: Ecosystem, name: &str) -> PathBuf {
    local_package_dir(root, org, ecosystem, name).join(DOCUMENT_FILE)
}

/// Where the archive stored under `key` is, below its namespace:
/// `archives/<shard>/<key>`, the shard being the first two hex digits of the
/// key's digest.
#[must_use]
pub fn archive_object_path(key: &str) -> String {
    let digest = key.split_once('-').map_or(key, |(_, digest)| digest);
    let shard = digest.get(..2).unwrap_or("00");
    format!("{ARCHIVES_DIR}/{shard}/{key}")
}

/// Where the archive stored under `key` in `(org, ecosystem)`'s namespace of
/// the local hosted store at `root` is.
#[must_use]
pub fn local_archive_path(root: &Path, org: &str, ecosystem: Ecosystem, key: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(namespace_segments(org, ecosystem));
    path.extend(archive_object_path(key).split('/'));
    path
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

/// The directories of package `name` in `ecosystem` below `packages/`.
///
/// Every non-leaf segment starts with `@` and the leaf segment does not, so no
/// package directory can be an ancestor of another.
#[must_use]
pub fn package_segments(ecosystem: Ecosystem, name: &str) -> Vec<String> {
    match ecosystem {
        Ecosystem::Npm => match name
            .strip_prefix(MARKER)
            .and_then(|rest| rest.split_once('/'))
        {
            Some((scope, rest)) if !scope.is_empty() && !rest.is_empty() => {
                vec![format!("{MARKER}{}", encode_segment(scope)), encode_segment(rest)]
            }
            _ => vec![MARKER.to_string(), encode_segment(name)],
        },
        Ecosystem::Cargo | Ecosystem::Pypi => vec![MARKER.to_string(), encode_segment(name)],
        Ecosystem::Oci => {
            if name.contains('/')
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
            {
                let mut parts: Vec<&str> = name.split('/').collect();
                let leaf = parts.pop().unwrap_or_default();
                let mut segments: Vec<String> = parts
                    .into_iter()
                    .map(|part| format!("{MARKER}{}", encode_segment(part)))
                    .collect();
                segments.push(encode_segment(leaf));
                segments
            } else {
                vec![MARKER.to_string(), encode_segment(name)]
            }
        }
    }
}

/// The segment of package `name`'s blob `file` below `blobs/`. A `.tgz`,
/// `.sigstore` or `.crate` file drops the `<basename>-` it starts with, so a
/// tarball's segment is its version and extension, or keeps all of its name
/// behind `@` when it does not start with one. Any other file, such as an
/// image digest or a wheel, is its name.
#[must_use]
pub fn blob_segment(name: &str, file: &str) -> String {
    if !is_versioned(file) {
        return encode_segment(file);
    }
    match strip_basename(name, file) {
        Some(rest) => encode_segment(rest),
        None => format!("{MARKER}{}", encode_segment(file)),
    }
}

/// The part of `file` after `name`'s `<basename>-`, unless it is empty or
/// starts with `.`, which would make it a dot segment or hidden file.
fn strip_basename<'file>(name: &str, file: &'file str) -> Option<&'file str> {
    let rest = file
        .strip_prefix(basename(name))?
        .strip_prefix('-')?;
    (!rest.is_empty() && !rest.starts_with('.')).then_some(rest)
}

fn is_versioned(file: &str) -> bool {
    VERSIONED_EXTENSIONS
        .iter()
        .any(|extension| file.ends_with(extension))
}

/// The last `/`-separated component of `name`, as
/// [`pnpr_package_name::CanonicalPackageName`] names it.
fn basename(name: &str) -> &str {
    name.rsplit_once('/').map_or(name, |(_, last)| last)
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
