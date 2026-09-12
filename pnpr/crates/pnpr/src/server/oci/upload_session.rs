use super::{
    BlobUpload, Body, CanonicalPackageName, Digest, ErrorCode, Method, RegistryError, Request,
    Response, StatusCode, Storage, accepted, append_body, created, error, hash_upload, header,
    hosted_read_namespace, method_not_allowed, no_content, parse_content_range, query_param,
    range_not_satisfiable, registry_error, upload_lock_key,
};

impl Request {
    pub(super) async fn mount_blob(
        &self,
        destination: &Storage,
        key: &CanonicalPackageName,
        mount: &str,
        from: &str,
    ) -> Result<Option<Response>, RegistryError> {
        let Ok(digest) = Digest::parse(mount) else { return Ok(None) };
        let Ok((source_key, source)) = self.hosted_source(from) else { return Ok(None) };
        if self.token_forbids_pull(source_key.as_str())? {
            return Ok(None);
        }
        let source_org = match hosted_read_namespace(
            &self.state,
            &self.identity,
            &source,
            source_key.as_str(),
        ) {
            Ok(org) => org,
            Err(
                RegistryError::Unauthenticated { .. }
                | RegistryError::Forbidden { .. }
                | RegistryError::NotFound,
            ) => return Ok(None),
            Err(err) => return Err(err),
        };
        let source_storage = self.state.inner.storage.for_hosted(&source_org);
        let Some((body, _)) =
            source_storage.open_hosted_blob(&source_key, &digest.blob_filename()).await?
        else {
            return Ok(None);
        };
        let upload = destination.begin_blob_upload(key).await?;
        if let Err(refusal) =
            append_body(destination, &upload, body, self.state.inner.config.oci.max_blob_bytes)
                .await
        {
            destination.abort_blob_upload(upload.id()).await?;
            return Ok(Some(refusal.respond()));
        }
        Ok(Some(self.finish_upload(destination, upload, key, mount).await))
    }

    /// `POST /v2/<name>/blobs/uploads/` — start an upload, or complete one in
    /// a single request when the client sends `?digest=`.
    pub(super) async fn start_upload(&self, name: &str, body: Body) -> Response {
        if self.method != Method::POST {
            return method_not_allowed();
        }
        let (key, org) = match self.publish_target(name) {
            Ok(target) => target,
            Err(refusal) => return refusal.respond(),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        if let (Some(mount), Some(from)) =
            (query_param(Some(&self.query), "mount"), query_param(Some(&self.query), "from"))
        {
            match self.mount_blob(&storage, &key, &mount, &from).await {
                Ok(Some(response)) => return response,
                Ok(None) => {}
                Err(err) => return registry_error(err),
            }
        }
        let upload = match storage.begin_blob_upload(&key).await {
            Ok(upload) => upload,
            Err(err) => return registry_error(err),
        };
        if let Err(refusal) =
            append_body(&storage, &upload, body, self.state.inner.config.oci.max_blob_bytes).await
        {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return refusal.respond();
        }
        match self.digest.as_deref() {
            Some(digest) => self.finish_upload(&storage, upload, &key, digest).await,
            None => self.upload_progress(&key, &upload).await,
        }
    }

    /// `PATCH`/`PUT`/`GET`/`DELETE /v2/<name>/blobs/uploads/<id>`.
    pub(super) async fn upload(&self, name: &str, id: &str, body: Body) -> Response {
        let (key, org) = match self.publish_target(name) {
            Ok(target) => target,
            Err(refusal) => return refusal.respond(),
        };
        // One upload is one sequence of bytes, so its requests are serialized:
        // two chunks appending at once, or a chunk landing between the hash
        // and the promotion, would store bytes that are not the digest they
        // are stored under.
        let _guard = self.state.inner.package_locks.lock(&upload_lock_key(id)).await;
        let storage = self.state.inner.storage.for_hosted(&org);
        let upload = match storage.open_blob_upload(&key, id).await {
            Ok(Some(upload)) => upload,
            Ok(None) => return error(ErrorCode::BlobUploadUnknown, "no such upload"),
            Err(err) => return registry_error(err),
        };
        match self.method {
            Method::PATCH => self.append_chunk(&storage, &key, &upload, body).await,
            Method::PUT => self.complete_upload(&storage, key, upload, body).await,
            Method::GET => self.upload_progress(&key, &upload).await,
            Method::DELETE => match storage.abort_blob_upload(upload.id()).await {
                Ok(_) => no_content(StatusCode::NO_CONTENT),
                Err(err) => registry_error(err),
            },
            _ => method_not_allowed(),
        }
    }

    /// Append one chunk of an upload and report the session's new offset.
    pub(super) async fn append_chunk(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        upload: &BlobUpload,
        body: Body,
    ) -> Response {
        if let Err(response) = self.check_chunk_start(key, upload).await {
            return response;
        }
        let appended =
            append_body(storage, upload, body, self.state.inner.config.oci.max_blob_bytes).await;
        match appended {
            Ok(()) => self.upload_progress(key, upload).await,
            Err(refusal) => refusal.respond(),
        }
    }

    /// Append the last bytes of an upload and promote it to the blob its
    /// digest names.
    pub(super) async fn complete_upload(
        &self,
        storage: &pnpr_storage::Storage,
        key: CanonicalPackageName,
        upload: BlobUpload,
        body: Body,
    ) -> Response {
        let Some(digest) = self.digest.as_deref() else {
            return error(ErrorCode::DigestInvalid, "a completed upload must name its digest");
        };
        let appended =
            append_body(storage, &upload, body, self.state.inner.config.oci.max_blob_bytes).await;
        if let Err(refusal) = appended {
            return refusal.respond();
        }
        self.finish_upload(storage, upload, &key, digest).await
    }

    /// Refuse a chunk that does not continue where the upload left off, so a
    /// retry cannot silently interleave bytes.
    pub(super) async fn check_chunk_start(
        &self,
        key: &CanonicalPackageName,
        upload: &BlobUpload,
    ) -> Result<(), Response> {
        let Some(range) = self.headers.get(header::CONTENT_RANGE) else { return Ok(()) };
        let Some((start, end)) = range.to_str().ok().and_then(parse_content_range) else {
            return Err(error(ErrorCode::BlobUploadInvalid, "malformed Content-Range"));
        };
        // A body of a different length than the range declares would leave
        // the upload somewhere neither side named. Checked against the
        // declared length before anything is written, rather than against
        // where the upload ended up afterwards.
        //
        // The span is computed with a ceiling rather than plain arithmetic:
        // `0-18446744073709551615` is a range a client can send, and one more
        // than it does not fit the number that holds it.
        let Some(span) = end.checked_sub(start).and_then(|span| span.checked_add(1)) else {
            return Err(error(ErrorCode::BlobUploadInvalid, "Content-Range is not a real span"));
        };
        let limit = self.state.inner.config.oci.max_blob_bytes;
        if span > limit {
            return Err(error(
                ErrorCode::SizeInvalid,
                format!("a blob may not exceed {limit} bytes"),
            ));
        }
        if let Some(declared) = self.content_length()
            && declared != span
        {
            return Err(error(
                ErrorCode::BlobUploadInvalid,
                "Content-Length disagrees with Content-Range",
            ));
        }
        let offset = upload.offset().await.map_err(registry_error)?;
        if start == offset {
            return Ok(());
        }
        // The refusal carries where the upload actually stands, so the client
        // can resume rather than start over.
        Err(range_not_satisfiable(&self.base, key.as_str(), upload.id(), offset))
    }

    pub(super) fn content_length(&self) -> Option<u64> {
        let declared = self.headers.get(header::CONTENT_LENGTH)?.to_str().ok()?;
        declared.trim().parse().ok()
    }

    pub(super) async fn upload_progress(
        &self,
        key: &CanonicalPackageName,
        upload: &BlobUpload,
    ) -> Response {
        match upload.offset().await {
            Ok(offset) => accepted(&self.base, key.as_str(), upload.id(), offset),
            Err(err) => registry_error(err),
        }
    }

    /// Verify a finished upload against the digest the client promised and
    /// promote it into the repository.
    pub(super) async fn finish_upload(
        &self,
        storage: &Storage,
        upload: BlobUpload,
        key: &CanonicalPackageName,
        digest: &str,
    ) -> Response {
        let Ok(digest) = Digest::parse(digest) else {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        match hash_upload(&upload).await {
            Ok(actual) if actual == digest => {}
            Ok(_) => {
                let _ = storage.abort_blob_upload(upload.id()).await;
                return error(ErrorCode::DigestInvalid, "uploaded bytes do not match the digest");
            }
            Err(err) => return registry_error(err),
        }
        match storage.finalize_uploaded_blob(upload, key, &digest.blob_filename()).await {
            Ok(pnpr_storage::BlobFinalize::Conflict) => {
                error(ErrorCode::DigestInvalid, "stored blob conflicts with the uploaded content")
            }
            Ok(_) => created(&format!("{}/{}/blobs/{digest}", self.base, key.as_str()), &digest),
            Err(err) => registry_error(err),
        }
    }
}
