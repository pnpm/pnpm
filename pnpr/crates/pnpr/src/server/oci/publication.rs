use super::{
    Digest, ErrorCode, ImageDocument, MAX_MANIFEST_REFERENCES, Manifest, ManifestEntry, Refusal,
    now_millis,
};
use crate::server::{
    Action, AppState, Identity, RegistrySource, authorize,
    documents::stage_hosted_artifact,
    publishing::{PublishTarget, StagedPublish, resolve_publish_target_for},
};
use axum::body::Bytes;
use pnpr_error::RegistryError;
use pnpr_oci::TagEntry;
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use std::collections::HashSet;

pub(in crate::server) struct OciPublication {
    pub(super) key: CanonicalPackageName,
    org: String,
    bytes: Bytes,
    manifest: Manifest,
    pub(super) digest: Digest,
    reference: String,
}

pub(in crate::server) fn authorize_publication(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &str,
) -> Result<(CanonicalPackageName, String), Refusal> {
    let key = CanonicalPackageName::parse(name, Ecosystem::Oci)
        .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "not a valid repository name"))?;
    match resolve_publish_target_for(state, identity, registry, Ecosystem::Oci, key.as_str()) {
        PublishTarget::Hosted { source, org } => {
            authorize(
                state,
                identity,
                &RegistrySource::Hosted(source),
                key.as_str(),
                Action::Publish,
            )?;
            Ok((key, org))
        }
        PublishTarget::Denied(err) => Err(err.into()),
        PublishTarget::Reject(reason) => Err(Refusal::new(ErrorCode::Denied, reason)),
        PublishTarget::NotFound => Err(super::unknown_repository(name)),
    }
}

impl OciPublication {
    pub(in crate::server) fn new(
        target: (CanonicalPackageName, String),
        reference: String,
        bytes: Bytes,
        content_type: Option<&str>,
        limit: usize,
    ) -> Result<Self, Refusal> {
        if bytes.len() > limit {
            return Err(Refusal::new(ErrorCode::SizeInvalid, "manifest is too large"));
        }
        let digest = Digest::of(&bytes);
        match Digest::parse(&reference) {
            Ok(addressed) if addressed != digest => {
                return Err(Refusal::new(
                    ErrorCode::DigestInvalid,
                    "manifest does not match the digest it was pushed under",
                ));
            }
            Err(_) if !pnpr_oci::is_valid_tag(&reference) => {
                return Err(Refusal::new(ErrorCode::ManifestInvalid, "not a valid tag or digest"));
            }
            _ => {}
        }
        let manifest = Manifest::parse(&bytes, content_type)
            .map_err(|err| Refusal::new(ErrorCode::ManifestInvalid, err.to_string()))?;
        Ok(Self { key: target.0, org: target.1, bytes, manifest, digest, reference })
    }

    pub(in crate::server) fn key(&self) -> &CanonicalPackageName {
        &self.key
    }

    pub(super) fn subject(&self) -> Option<Digest> {
        self.manifest.referrer_metadata().subject
    }

    pub(in crate::server) async fn stage(self, state: &AppState) -> Result<StagedPublish, Refusal> {
        let storage = state.inner.storage.for_hosted(&self.org);
        let document = storage.read_hosted_document(&self.key).await?;
        let snapshot = document
            .map(|bytes| ImageDocument::parse(&bytes))
            .transpose()
            .map_err(RegistryError::from)?
            .unwrap_or_default();
        if snapshot.deleting_blob.is_some() {
            return Err(RegistryError::DocumentWriteConflict {
                package: self.key.as_str().to_string(),
            }
            .into());
        }
        self.check_referenced_blobs(&storage).await?;
        let addition = self.document_addition(snapshot.generation);
        let children: Vec<Digest> = if pnpr_oci::media_type::is_index(self.manifest.media_type()) {
            self.manifest.references().map(|descriptor| descriptor.digest.clone()).collect()
        } else {
            Vec::new()
        };
        let refuse = |stored: &ImageDocument| {
            refuse_moved_document(stored, snapshot.generation, self.key.as_str(), &children)
        };
        stage_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.digest.blob_filename(),
            &self.bytes,
            &refuse,
            addition,
        )
        .await
        .map_err(Into::into)
    }
    fn document_addition(&self, generation: u64) -> ImageDocument {
        let mut addition = ImageDocument::new(self.key.as_str());
        addition.generation = generation;
        addition.insert_manifest(ManifestEntry {
            digest: self.digest.clone(),
            media_type: self.manifest.media_type().to_string(),
            size: self.bytes.len() as u64,
            referrer: Some(self.manifest.referrer_metadata()),
        });
        if Digest::parse(&self.reference).is_err() {
            addition.set_tag(TagEntry {
                tag: self.reference.clone(),
                digest: self.digest.clone(),
                updated: now_millis(),
            });
        }
        addition
    }

    /// Every blob the manifest references must already be in this repository,
    /// at the size the manifest declares.
    async fn check_referenced_blobs(&self, storage: &pnpr_storage::Storage) -> Result<(), Refusal> {
        let mut looked_up = HashSet::new();
        for descriptor in self.manifest.references() {
            if !looked_up.insert(descriptor.digest.clone()) {
                continue;
            }
            if looked_up.len() > MAX_MANIFEST_REFERENCES {
                return Err(Refusal::new(
                    ErrorCode::ManifestInvalid,
                    format!(
                        "a manifest may not reference more than {MAX_MANIFEST_REFERENCES} blobs",
                    ),
                ));
            }
            let stored =
                storage.open_hosted_blob(&self.key, &descriptor.digest.blob_filename()).await?;
            match stored {
                Some((_, Some(size))) if size != descriptor.size => {
                    return Err(Refusal::new(
                        ErrorCode::ManifestInvalid,
                        format!(
                            "{} is {size} bytes, but the manifest declares {}",
                            descriptor.digest, descriptor.size,
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(Refusal::new(
                        ErrorCode::ManifestBlobUnknown,
                        format!("{} is not in this repository", descriptor.digest),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A staged manifest only lands on the document it was built against, with
/// every child manifest it names still present.
fn refuse_moved_document(
    stored: &ImageDocument,
    generation: u64,
    package: &str,
    children: &[Digest],
) -> Result<(), RegistryError> {
    if stored.generation != generation || stored.deleting_blob.is_some() {
        return Err(RegistryError::DocumentWriteConflict { package: package.to_string() });
    }
    for child in children {
        if stored.manifest(child).is_none() {
            return Err(RegistryError::BadRequest {
                reason: format!("{child} is not a manifest in this repository"),
            });
        }
    }
    Ok(())
}
