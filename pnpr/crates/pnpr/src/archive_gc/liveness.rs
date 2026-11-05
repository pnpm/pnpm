//! Which image blobs of a repository its retained manifests reach.

use crate::{RegistryError, Result};
use pnpr_oci::{Descriptor, Digest, ImageDocument, Manifest, media_type};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::Storage;
use std::collections::HashSet;

pub(crate) fn parse_image_document(body: &[u8]) -> Result<ImageDocument> {
    let document = ImageDocument::parse(body)?;
    if document.format_version > pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: "image document".to_string(),
            format_version: document.format_version,
            supported_version: pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION,
        });
    }
    Ok(document)
}

/// Every blob a manifest `document` retains reaches, the manifests
/// included. A retained manifest that is missing or corrupt is an error,
/// so nothing it might have reached is collected.
pub(crate) async fn referenced_document_blobs(
    storage: &Storage,
    name: &CanonicalPackageName,
    document: &ImageDocument,
    manifest_limit: usize,
) -> Result<HashSet<String>> {
    let mut pending: Vec<(Digest, Option<String>)> = document
        .manifests()
        .iter()
        .map(|entry| (entry.digest.clone(), Some(entry.media_type.clone())))
        .collect();
    let mut visited = HashSet::new();
    let mut reachable = HashSet::new();
    while let Some((digest, content_type)) = pending.pop() {
        if !visited.insert(digest.clone()) {
            continue;
        }
        let filename = digest.blob_filename();
        reachable.insert(filename.clone());
        let manifest =
            read_manifest_blob(storage, name, &digest, content_type.as_deref(), manifest_limit)
                .await?;
        for reference in manifest.references() {
            reachable.insert(reference.digest.blob_filename());
            if media_type::is_index(manifest.media_type()) {
                let content_type = resolve_reference_media_type(reference, document);
                pending.push((reference.digest.clone(), content_type));
            }
        }
    }
    Ok(reachable)
}

fn resolve_reference_media_type(
    reference: &Descriptor,
    document: &ImageDocument,
) -> Option<String> {
    reference.media_type
        .clone()
        .or_else(|| {
            document.manifest(&reference.digest).map(|entry| entry.media_type.clone())
        })
}

/// Read one retained manifest back and parse it, checking it is the blob its
/// digest names.
async fn read_manifest_blob(
    storage: &Storage,
    name: &CanonicalPackageName,
    digest: &Digest,
    content_type: Option<&str>,
    manifest_limit: usize,
) -> Result<Manifest> {
    let filename = digest.blob_filename();
    let invalid = |reason: String| RegistryError::BadRequest {
        reason: format!("cannot collect {}/{filename}: {reason}", name.as_str()),
    };
    let (body, _) = storage
        .open_hosted_blob(name, &filename)
        .await?
        .ok_or_else(|| invalid("retained manifest is missing".into()))?;
    let bytes = axum::body::to_bytes(body, manifest_limit).await
        .map_err(|error| invalid(error.to_string()))?;
    if Digest::of(&bytes) != *digest {
        return Err(invalid("manifest digest mismatch".into()));
    }
    Manifest::parse(&bytes, content_type).map_err(|error| invalid(error.to_string()))
}
