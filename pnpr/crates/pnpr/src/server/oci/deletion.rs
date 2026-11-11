use super::{Digest, ErrorCode, ImageDocument, Request, error, no_content, registry_error};
use crate::archive_gc::liveness::referenced_document_blobs;
use axum::{http::StatusCode, response::Response};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, metadata::BlobLookup};

impl Request {
    pub(super) async fn delete_blob(&self, name: &str, digest: &Digest) -> Response {
        let repo = match self.hosted_repo(name) {
            Ok(repo) => repo,
            Err(refusal) => return refusal.respond(),
        };
        let (key, storage) = (&repo.key, &repo.storage);
        let _guard = self.state.inner.locks.packages.lock(key.as_str()).await;
        if let Err(err) = self.authorize_unpublish(&repo).await {
            return registry_error(err);
        }
        let result = async {
            for _ in 0..DOCUMENT_WRITE_RETRIES {
                let Some(response) = self.try_delete_blob(storage, key, digest).await? else {
                    continue;
                };
                if response.status().is_success() {
                    self.log_oci_blob_deleted(key, digest);
                }
                return Ok(response);
            }
            Err(RegistryError::DocumentWriteConflict { package: key.as_str().to_string() })
        }
        .await;
        result.unwrap_or_else(registry_error)
    }

    fn log_oci_blob_deleted(&self, repository: &CanonicalPackageName, digest: &Digest) {
        let actor = self.identity.username().unwrap_or("-");
        tracing::info!(
            target: "pnpr::audit",
            action = "oci.delete_blob",
            actor = %actor,
            repository = %repository.as_str(),
            digest = %digest,
            "oci blob deleted",
        );
    }

    /// One attempt at deleting the blob: dropping the repository's mapping
    /// of it, which holds only while the repository's document is the one it
    /// was found unreferenced in, and reclaiming its bytes unless another
    /// repository maps them. `None` means another writer moved the document
    /// first and the attempt should be repeated.
    async fn try_delete_blob(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        digest: &Digest,
    ) -> Result<Option<Response>, RegistryError> {
        let filename = digest.blob_filename();
        let lookup = BlobLookup { ids: Vec::new(), artifacts: vec![filename.clone()] };
        let base = storage.read_hosted_document_for_publish(key, &lookup).await?;
        let document =
            parse_deletion_target(base.stored.as_ref().map(|stored| stored.bytes.as_slice()), key)?;
        if base.artifacts.is_empty() {
            return Ok(Some(error(ErrorCode::BlobUnknown, "no such blob")));
        }
        let reachable = referenced_document_blobs(
            storage,
            key,
            &document,
            self.state.inner.config.http.oci.max_manifest_bytes,
        )
        .await?;
        if reachable.contains(&filename) {
            return Ok(Some(error(
                ErrorCode::Denied,
                format!("{digest} is referenced by a retained manifest"),
            )));
        }
        let expected = base.package_generation();
        match storage.unmap_hosted_artifacts(key, expected, vec![filename]).await? {
            Some(_) => Ok(Some(no_content(StatusCode::ACCEPTED))),
            None => Ok(None),
        }
    }
}

fn parse_deletion_target(
    existing_bytes: Option<&[u8]>,
    key: &CanonicalPackageName,
) -> Result<ImageDocument, RegistryError> {
    let document = existing_bytes
        .map(ImageDocument::parse)
        .transpose()?
        .unwrap_or_else(|| ImageDocument::new(key.as_str()));
    if document.format_version > pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.as_str().to_string(),
            format_version: document.format_version,
            supported_version: pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION,
        });
    }
    Ok(document)
}
