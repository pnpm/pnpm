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

mod digest;
mod document;
mod error_body;
mod manifest;

pub use digest::{Digest, DigestError};
pub use document::{ImageDocument, ManifestEntry, TagEntry};
pub use error_body::{ErrorBody, ErrorCode};
pub use manifest::{Descriptor, Manifest, ManifestError};

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
