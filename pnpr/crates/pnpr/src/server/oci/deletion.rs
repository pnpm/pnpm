use super::{Digest, ErrorCode, ImageDocument, Request, error, no_content, registry_error};
use crate::{
    oci_maintenance::referenced_document_blobs,
    server::{Action, RegistrySource, authorize},
};
use axum::{http::StatusCode, response::Response};
use pnpr_error::RegistryError;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentWrite};

impl Request {
    pub(super) async fn delete_blob(&self, name: &str, digest: &Digest) -> Response {
        let (key, source) = match self.hosted_source(name) {
            Ok(target) => target,
            Err(refusal) => return refusal.respond(),
        };
        let org = match crate::server::hosted_read_namespace(
            &self.state,
            &self.identity,
            &source,
            key.as_str(),
        ) {
            Ok(org) => org,
            Err(err) => return registry_error(err),
        };
        if let Err(err) = authorize(
            &self.state,
            &self.identity,
            &RegistrySource::Hosted(source.clone()),
            key.as_str(),
            Action::Unpublish,
        ) {
            return registry_error(err);
        }
        let storage = self.state.inner.storage.for_hosted(&org);
        let _guard = self.state.inner.package_locks.lock(key.as_str()).await;
        let result = async {
            for _ in 0..DOCUMENT_WRITE_RETRIES {
                let snapshot = storage.read_hosted_document_for_update(&key).await?;
                let mut document = snapshot
                    .as_ref()
                    .map(|stored| ImageDocument::parse(&stored.bytes))
                    .transpose()?
                    .unwrap_or_else(|| ImageDocument::new(key.as_str()));
                if document.deleting_blob.is_some() {
                    return Err(RegistryError::DocumentWriteConflict {
                        package: key.as_str().to_string(),
                    });
                }
                if storage.open_hosted_blob(&key, &digest.blob_filename()).await?.is_none() {
                    return Ok(error(ErrorCode::BlobUnknown, "no such blob"));
                }
                if referenced_document_blobs(
                    &storage,
                    &key,
                    &document,
                    self.state.inner.config.oci.max_manifest_bytes,
                )
                .await?
                .contains(&digest.blob_filename())
                {
                    return Err(RegistryError::BadRequest {
                        reason: format!("{digest} is referenced by a retained manifest"),
                    });
                }
                document.generation =
                    document.generation.checked_add(1).ok_or_else(|| RegistryError::Internal {
                        reason: "OCI document generation exhausted".to_string(),
                    })?;
                document.deleting_blob = Some(digest.clone());
                if storage
                    .write_hosted_document_if_current(
                        &key,
                        &document.to_bytes(),
                        snapshot.as_ref().map(|stored| &stored.version),
                    )
                    .await?
                    == DocumentWrite::Conflict
                {
                    continue;
                }
                storage.remove_hosted_blob(&key, &digest.blob_filename()).await?;
                storage
                    .update_hosted_document_with_retry(&key, DOCUMENT_WRITE_RETRIES, |bytes| {
                        let mut document = ImageDocument::parse(bytes.ok_or_else(|| {
                            RegistryError::Internal {
                                reason: "OCI deletion document disappeared".to_string(),
                            }
                        })?)?;
                        if document.deleting_blob.as_ref() != Some(digest) {
                            return Err(RegistryError::DocumentWriteConflict {
                                package: key.as_str().to_string(),
                            });
                        }
                        document.deleting_blob = None;
                        Ok(Some(document.to_bytes()))
                    })
                    .await?;
                return Ok(no_content(StatusCode::ACCEPTED));
            }
            Err(RegistryError::DocumentWriteConflict { package: key.as_str().to_string() })
        }
        .await;
        result.unwrap_or_else(registry_error)
    }
}
