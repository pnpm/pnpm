//! The OCI distribution protocol pnpr speaks.
//!
//! Three things live here, all of them pure data: the [`Digest`] that names
//! every blob, the manifest shapes a client pushes ([`Manifest`]), and the
//! [`ImageDocument`] a hosted registry keeps per repository.
//!
//! A repository's blobs are content-addressed and immutable, so pushing one
//! is not what makes a release visible — referencing it from a manifest is.
//! That is why the document records manifests and tags but not layers: the
//! manifest write is the commit point, and a blob no manifest names is
//! garbage for collection rather than a half-published release.

pub mod media_type;

pub use digest::{Digest, DigestError};
pub use document::{ImageDocument, ManifestEntry, ReferrerMetadata, TagEntry};
pub use error_body::{ErrorBody, ErrorCode};
pub use manifest::{Descriptor, Manifest, ManifestError};

mod digest;
mod document;
mod error_body;
mod manifest;

/// The longest tag the distribution spec admits.
pub const MAX_TAG_LEN: usize = 128;

/// Whether `tag` matches the spec's tag grammar,
/// `[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}`.
///
/// A manifest reference is a tag or a digest and nothing else, so anything
/// that is neither has to be refused: stored verbatim it would be metadata no
/// conforming client could address, and a digest-shaped near-miss like
/// `sha256:short` would sit in the tag list looking like a digest.
#[must_use]
pub fn is_valid_tag(tag: &str) -> bool {
    let mut characters = tag.chars();
    let Some(first) = characters.next() else { return false };
    tag.len() <= MAX_TAG_LEN
        && (first.is_ascii_alphanumeric() || first == '_')
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

/// The path segment every distribution endpoint sits under. Clients derive it
/// from the image reference's host, so it cannot be moved or renamed.
pub const API_SEGMENT: &str = "v2";

/// The `docker` and `oci` media types a manifest may carry, and the ones a
/// client may ask for by `Accept`.
pub const MANIFEST_MEDIA_TYPES: &[&str] = &[
    media_type::OCI_IMAGE_MANIFEST,
    media_type::OCI_IMAGE_INDEX,
    media_type::DOCKER_IMAGE_MANIFEST,
    media_type::DOCKER_MANIFEST_LIST,
];

#[cfg(test)]
mod tests;
