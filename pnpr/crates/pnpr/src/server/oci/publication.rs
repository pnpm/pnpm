use super::{
    Digest, ErrorCode, ImageDocument, MAX_MANIFEST_REFERENCES, Manifest, ManifestEntry, Refusal,
    now_millis,
};
use crate::server::{
    Action, AppState, Identity, RegistrySource, authorize_continuation, authorize_package,
    documents::{HostedArtifact, HostedTarget, stage_hosted_artifact_on},
    publishing::{PublishTarget, StagedPublish, resolve_publish_target_for},
};
use axum::body::Bytes;
use pnpr_error::RegistryError;
use pnpr_oci::TagEntry;
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    BlobId,
    metadata::{ArtifactBlob, BlobLookup, BlobRef, BlobState, MappedArtifact},
};
use std::collections::HashSet;

pub(in crate::server) struct OciPublication {
    pub(super) key: CanonicalPackageName,
    namespace: String,
    /// The hosted registry whose `publish` rule admitted [`Self::publisher`].
    registry: String,
    publisher: Identity,
    bytes: Bytes,
    manifest: Manifest,
    pub(super) digest: Digest,
    reference: String,
}

/// The repository a push writes to, once the caller is allowed to publish it.
pub(in crate::server) struct OciTarget {
    pub(in crate::server) key: CanonicalPackageName,
    pub(in crate::server) namespace: String,
    /// The hosted registry whose `publish` rule admitted [`Self::publisher`].
    pub(in crate::server) registry: String,
    pub(in crate::server) publisher: Identity,
}

pub(in crate::server) async fn authorize_publication(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &str,
) -> Result<OciTarget, Refusal> {
    let target = publication_target(state, identity, registry, name)?;
    let hosted = RegistrySource::Hosted(target.registry.clone());
    let (key, namespace) = (target.key.as_str(), Some(target.namespace.as_str()));
    authorize_package(state, identity, &hosted, key, Action::Publish, namespace).await?;
    Ok(target)
}

/// The repository a later request of an upload session writes to. The
/// request that opened the session passed [`authorize_publication`].
pub(in crate::server) fn continue_publication(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &str,
) -> Result<OciTarget, Refusal> {
    let target = publication_target(state, identity, registry, name)?;
    let hosted = RegistrySource::Hosted(target.registry.clone());
    authorize_continuation(state, identity, &hosted, target.key.as_str(), Action::Publish)?;
    Ok(target)
}

/// The hosted repository a push of `name` lands on, before any `publish` check.
fn publication_target(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &str,
) -> Result<OciTarget, Refusal> {
    let key = CanonicalPackageName::parse(name, Ecosystem::Oci)
        .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "not a valid repository name"))?;
    match resolve_publish_target_for(state, identity, registry, Ecosystem::Oci, key.as_str()) {
        PublishTarget::Hosted { source, namespace } => {
            Ok(OciTarget { key, namespace, registry: source, publisher: identity.clone() })
        }
        PublishTarget::Denied(err) => Err(err.into()),
        PublishTarget::Reject(reason) => Err(Refusal::new(ErrorCode::Denied, reason)),
        PublishTarget::NotFound => Err(super::unknown_repository(name)),
    }
}

impl OciPublication {
    pub(in crate::server) fn new(
        target: OciTarget,
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
        let OciTarget { key, namespace, registry, publisher } = target;
        Ok(Self { key, namespace, registry, publisher, bytes, manifest, digest, reference })
    }

    pub(in crate::server) fn key(&self) -> &CanonicalPackageName {
        &self.key
    }

    /// The hosted namespace the image is published into.
    pub(in crate::server) fn namespace(&self) -> &str {
        &self.namespace
    }

    pub(super) fn subject(&self) -> Option<Digest> {
        self.manifest.referrer_metadata().subject
    }

    pub(in crate::server) async fn stage(self, state: &AppState) -> Result<StagedPublish, Refusal> {
        let storage = state.inner.storage.for_hosted(&self.namespace, Ecosystem::Oci);
        // The blobs the manifest references are read with the document, and
        // the commit is refused if a reclaim takes one of them after this.
        let referenced = self.referenced_digests()?;
        let lookup = BlobLookup {
            ids: vec![BlobId::of_bytes(Ecosystem::Oci, &self.bytes)],
            artifacts: referenced
                .iter()
                .map(Digest::blob_filename)
                .collect(),
        };
        let mut base = storage.read_hosted_document_for_publish(&self.key, &lookup).await?;
        let references = self.check_referenced_blobs(&referenced, &mut base.artifacts)?;
        let children = if pnpr_oci::media_type::is_index(self.manifest.media_type()) {
            referenced
        } else {
            Vec::new()
        };
        let refuse = |stored: &ImageDocument| refuse_missing_children(stored, &children);
        let addition = self.document_addition(children.clone());
        let filename = self.digest.blob_filename();
        let artifact = HostedArtifact { filename: &filename, bytes: &self.bytes, references };
        let target = HostedTarget {
            namespace: &self.namespace,
            registry: &self.registry,
            publisher: &self.publisher,
        };
        stage_hosted_artifact_on(state, &target, &self.key, artifact, base, &refuse, addition)
            .await
            .map_err(Into::into)
    }
    fn document_addition(&self, children: Vec<Digest>) -> ImageDocument {
        let mut addition = ImageDocument::new(self.key.as_str());
        addition.insert_manifest(ManifestEntry {
            digest: self.digest.clone(),
            media_type: self.manifest.media_type().to_string(),
            size: self.bytes.len() as u64,
            children,
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

    /// The distinct blobs the manifest references, at most
    /// [`MAX_MANIFEST_REFERENCES`] of them.
    fn referenced_digests(&self) -> Result<Vec<Digest>, Refusal> {
        let mut seen = HashSet::new();
        let mut digests = Vec::new();
        for descriptor in self.manifest.references() {
            if !seen.insert(descriptor.digest.clone()) {
                continue;
            }
            if digests.len() == MAX_MANIFEST_REFERENCES {
                return Err(Refusal::new(
                    ErrorCode::ManifestInvalid,
                    format!(
                        "a manifest may not reference more than {MAX_MANIFEST_REFERENCES} blobs",
                    ),
                ));
            }
            digests.push(descriptor.digest.clone());
        }
        Ok(digests)
    }

    /// Every blob the manifest references must already be in this repository,
    /// live and at the size the manifest declares. Returns them with the
    /// blobs holding their bytes, which the commit maps and depends on.
    /// `mapped` holds what the publish read of the repository's files.
    fn check_referenced_blobs(
        &self,
        referenced: &[Digest],
        mapped: &mut Vec<MappedArtifact>,
    ) -> Result<Vec<ArtifactBlob>, Refusal> {
        let mut references = Vec::with_capacity(referenced.len());
        for digest in referenced {
            let filename = digest.blob_filename();
            let found = mapped
                .iter()
                .position(|mapped| {
                    mapped.artifact == filename && mapped.row.state == BlobState::Live
                })
                .map(|index| mapped.swap_remove(index));
            let Some(found) = found else {
                return Err(Refusal::new(
                    ErrorCode::ManifestBlobUnknown,
                    format!("{digest} is not in this repository"),
                ));
            };
            for descriptor in self.manifest
                .references()
                .filter(|reference| reference.digest == *digest)
            {
                if descriptor.size != found.row.size {
                    return Err(Refusal::new(
                        ErrorCode::ManifestInvalid,
                        format!(
                            "{digest} is {} bytes, but the manifest declares {}",
                            found.row.size, descriptor.size,
                        ),
                    ));
                }
            }
            references.push(ArtifactBlob {
                ecosystem: Ecosystem::Oci,
                name: self.key.as_str().to_string(),
                version: String::new(),
                artifact: filename,
                blob: BlobRef { id: found.id, incarnation: found.row.incarnation },
                size: found.row.size,
            });
        }
        Ok(references)
    }
}

/// A staged index only lands on a document that holds every child manifest
/// it names.
fn refuse_missing_children(
    stored: &ImageDocument,
    children: &[Digest],
) -> Result<(), RegistryError> {
    for child in children {
        if stored.manifest(child).is_none() {
            return Err(RegistryError::BadRequest {
                reason: format!("{child} is not a manifest in this repository"),
            });
        }
    }
    Ok(())
}
