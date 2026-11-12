use super::{
    Action, Digest, ErrorCode, HostedRepo, ImageDocument, Ownership, RegistrySource, Request,
    authorize_with_ownership, error, no_content, registry_error,
};
use crate::archive_gc::liveness::referenced_document_blobs;
use axum::{http::StatusCode, response::Response};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{
    DOCUMENT_WRITE_RETRIES, metadata::BlobLookup, wait_after_document_write_conflict,
};

impl Request {
    pub(super) async fn delete_blob(&self, name: &str, digest: &Digest) -> Response {
        let repo = match self.hosted_repo(name) {
            Ok(repo) => repo,
            Err(refusal) => return refusal.respond(),
        };
        let key = &repo.key;
        let _guard = self.state.inner.locks.packages.lock(key.as_str()).await;
        let result = async {
            for attempt in 0..DOCUMENT_WRITE_RETRIES {
                let Some(response) = self.try_delete_blob(&repo, digest).await? else {
                    if attempt + 1 < DOCUMENT_WRITE_RETRIES {
                        wait_after_document_write_conflict(attempt).await;
                    }
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
        let actor = crate::audit::Actor::of(&self.identity);
        crate::audit!(
            actor,
            "oci.delete_blob",
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
        repo: &HostedRepo,
        digest: &Digest,
    ) -> Result<Option<Response>, RegistryError> {
        let (key, storage) = (&repo.key, &repo.storage);
        let filename = digest.blob_filename();
        let lookup = BlobLookup { ids: Vec::new(), artifacts: vec![filename.clone()] };
        let base = storage.read_hosted_document_for_publish(key, &lookup).await?;
        let document = self.authorize_blob_deletion(repo, base.stored.as_ref()).await?;
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

    async fn authorize_blob_deletion(
        &self,
        repo: &HostedRepo,
        stored: Option<&pnpr_storage::HostedDocumentForUpdate>,
    ) -> Result<ImageDocument, RegistryError> {
        let Some(stored) = stored else {
            self.authorize_unpublish(repo).await?;
            return Ok(ImageDocument::new(repo.key.as_str()));
        };
        let document = ImageDocument::parse(&stored.bytes)?;
        let source = RegistrySource::Hosted(repo.source.clone());
        let ownership = Ownership::Owners(&document.owners);
        authorize_with_ownership(
            &self.state,
            &self.identity,
            &source,
            (repo.key.as_str(), &ownership),
            Action::Unpublish,
        )?;
        check_deletion_target(document, &repo.key)
    }
}

fn check_deletion_target(
    document: ImageDocument,
    key: &CanonicalPackageName,
) -> Result<ImageDocument, RegistryError> {
    if document.format_version > pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.as_str().to_string(),
            format_version: document.format_version,
            supported_version: pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION,
        });
    }
    Ok(document)
}
