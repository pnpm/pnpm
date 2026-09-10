use super::{
    Action, Body, DOCUMENT_WRITE_RETRIES, Digest, DocumentUpdate, ErrorCode, ImageDocument, Method,
    OciPublication, Refusal, RegistryError, RegistrySource, Request, Response, StatusCode,
    authorize, collect_body, created, error, header, hosted_manifest_response, insert_header,
    method_not_allowed, no_content, read_hosted_document, read_manifest_bytes, registry_error,
    unknown_repository,
};

impl Request {
    /// `HEAD`/`GET`/`PUT`/`DELETE /v2/<name>/manifests/<reference>`.
    pub(super) async fn manifest(&self, name: &str, reference: &str, body: Body) -> Response {
        match self.method {
            Method::GET | Method::HEAD => self.read_manifest(name, reference).await,
            Method::PUT => self.write_manifest(name, reference, body).await,
            Method::DELETE => self.delete_manifest(name, reference).await,
            _ => method_not_allowed(),
        }
    }

    pub(super) async fn read_manifest(&self, name: &str, reference: &str) -> Response {
        if let Some((key, source)) = self.upstream_source(name) {
            return self.proxy_manifest(&key, &source, reference).await;
        }
        let repo = match self.hosted_repo(name) {
            Ok(repo) => repo,
            Err(refusal) => return refusal.respond(),
        };
        let document = match read_hosted_document::<ImageDocument>(
            &self.state,
            &self.identity,
            &repo.source,
            &repo.key,
        )
        .await
        {
            Ok(Some(document)) => document,
            Ok(None) => return unknown_repository(name).respond(),
            Err(err) => return registry_error(err),
        };
        let Some(entry) = document.resolve(reference).cloned() else {
            return error(ErrorCode::ManifestUnknown, "no such manifest or tag");
        };
        // A manifest is small enough to answer from memory, and the response
        // carries its digest and media type either way.
        let bytes = match read_manifest_bytes(
            &repo.storage,
            &repo.key,
            &entry.digest.blob_filename(),
            self.state.inner.config.oci.max_manifest_bytes,
        )
        .await
        {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return error(ErrorCode::ManifestUnknown, "no such manifest"),
            Err(err) => return registry_error(err),
        };
        let response = hosted_manifest_response(bytes, entry, self.method == Method::HEAD);
        self.caller_scoped(Some(repo.key.as_str()), response)
    }

    pub(super) async fn write_manifest(&self, name: &str, reference: &str, body: Body) -> Response {
        let publication = match self.manifest_publication(name, reference, body).await {
            Ok(publication) => publication,
            Err(refusal) => return refusal.respond(),
        };
        let digest = publication.digest.clone();
        let key = publication.key.clone();
        let subject = publication.subject();
        let _guard = self.state.inner.package_locks.lock(key.as_str()).await;
        let staged = match publication.stage(&self.state).await {
            Ok(staged) => staged,
            Err(refusal) => return refusal.respond(),
        };
        match super::super::publishing::commit_publishes(&self.state, vec![staged])
            .await
            .and_then(super::super::publishing::report_unrecorded)
        {
            Ok(()) => {
                let mut response =
                    created(&format!("{}/{}/manifests/{digest}", self.base, key.as_str()), &digest);
                if let Some(subject) = subject {
                    insert_header(&mut response, "oci-subject", &subject.to_string());
                }
                response
            }
            Err(err) => registry_error(err),
        }
    }

    pub(super) async fn delete_manifest(&self, name: &str, reference: &str) -> Response {
        let repo = match self.hosted_repo(name) {
            Ok(repo) => repo,
            Err(refusal) => return refusal.respond(),
        };
        if let Err(err) = authorize(
            &self.state,
            &self.identity,
            &RegistrySource::Hosted(repo.source.clone()),
            repo.key.as_str(),
            Action::Unpublish,
        ) {
            return registry_error(err);
        }
        let _guard = self.state.inner.package_locks.lock(repo.key.as_str()).await;
        let outcome = repo
            .storage
            .update_hosted_document_with_retry(&repo.key, DOCUMENT_WRITE_RETRIES, |existing| {
                let Some(bytes) = existing else { return Ok(None) };
                let mut document = ImageDocument::parse(bytes).map_err(RegistryError::Json)?;
                let removed = match Digest::parse(reference) {
                    Ok(digest) => document.remove_manifest(&digest),
                    Err(_) => document.remove_tag(reference),
                };
                Ok(removed.then(|| document.to_bytes()))
            })
            .await;
        match outcome {
            Ok(DocumentUpdate::Written) => no_content(StatusCode::ACCEPTED),
            Ok(_) => error(ErrorCode::ManifestUnknown, "no such manifest or tag"),
            Err(err) => registry_error(err),
        }
    }

    pub(super) async fn manifest_publication(
        &self,
        name: &str,
        reference: &str,
        body: Body,
    ) -> Result<OciPublication, Refusal> {
        let (key, org) = self.publish_target(name)?;
        let content_type =
            self.headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        let bytes = collect_body(body, self.state.inner.config.oci.max_manifest_bytes).await?;
        OciPublication::new(
            (key, org),
            reference.to_string(),
            bytes,
            content_type,
            self.state.inner.config.oci.max_manifest_bytes,
        )
    }
}
