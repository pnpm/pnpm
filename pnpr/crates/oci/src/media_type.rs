//! The media types the distribution protocol names.
//!
//! Docker's pre-OCI types are still what most clients push, so a registry
//! that only understood the `application/vnd.oci.*` spellings would reject
//! the common case.

pub const OCI_IMAGE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const DOCKER_IMAGE_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
pub const DOCKER_MANIFEST_LIST: &str = "application/vnd.docker.distribution.manifest.list.v2+json";

/// What a manifest with no `mediaType` of its own is served as.
pub const DEFAULT_MANIFEST: &str = OCI_IMAGE_MANIFEST;

/// Whether `media_type` names a manifest that lists other manifests rather
/// than a config and layers.
#[must_use]
pub fn is_index(media_type: &str) -> bool {
    matches!(media_type, OCI_IMAGE_INDEX | DOCKER_MANIFEST_LIST)
}
