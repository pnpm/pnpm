use super::{
    CanonicalPackageName, DOCUMENT_WRITE_RETRIES, Digest, ErrorCode, ImageDocument, Manifest,
    ManifestEntry, Method, ReferrerDescriptor, ReferrerFilter, ReferrerPage, ReferrerStep,
    Referrers, RegistryError, Request, Response, StatusCode, error, indexed_referrer,
    insert_header, json, media_type, method_not_allowed, query_param, read_hosted_document,
    read_image_document, read_manifest_bytes, referrer_entry_step, registry_error,
};

impl Request {
    pub(super) async fn referrers(&self, name: &str, digest: &str) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let Ok(digest) = Digest::parse(digest) else {
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        let Ok(last) =
            query_param(Some(&self.query), "last").map(|value| Digest::parse(&value)).transpose()
        else {
            return error(ErrorCode::DigestInvalid, "last must be a supported digest");
        };
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
            Ok(document) => document.unwrap_or_else(|| ImageDocument::new(repo.key.as_str())),
            Err(err) => return registry_error(err),
        };
        let filter = ReferrerFilter::new(digest, &self.query);
        let page =
            match self.scan_referrers(&repo.storage, &repo.key, &document, &filter, last).await {
                Ok(page) => page,
                Err(response) => return response,
            };
        if let Err(response) =
            self.record_referrer_index(&repo.storage, &repo.key, &page.additions).await
        {
            return response;
        }
        let response = self.referrers_page_response(&repo.key, &filter, &page);
        self.caller_scoped(Some(repo.key.as_str()), response)
    }

    /// Walk the manifest index from `last` onwards, reading only the manifests
    /// the index cannot answer for, until the page fills up.
    pub(super) async fn scan_referrers<'a>(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        document: &'a ImageDocument,
        filter: &ReferrerFilter,
        last: Option<Digest>,
    ) -> Result<ReferrerPage<'a>, Response> {
        let start = document.manifests().partition_point(|entry| {
            last.as_ref().is_some_and(|last| entry.digest.hex() <= last.hex())
        });
        let mut entries = document.manifests()[start..].iter().peekable();
        let mut page = ReferrerPage::new(self.state.inner.config.oci.max_manifest_bytes);
        // The index is migrated in place the first time a manifest is read for
        // metadata it should already carry. The re-read document is the one
        // every later entry is judged against, under a lock so two scans do
        // not migrate the same repository at once.
        let mut migration_guard = None;
        let mut migrated: Option<ImageDocument> = None;
        while let Some(&entry) = entries.peek() {
            let indexed = indexed_referrer(migrated.as_ref(), entry);
            // The lock is only held while the migration has something to
            // write; an entry the index already answers for releases it.
            if migration_guard.is_some() && indexed.flatten().is_some() && page.additions.is_empty()
            {
                drop(migration_guard.take());
            }
            match referrer_entry_step(entry, indexed, filter, &page, migration_guard.is_some()) {
                ReferrerStep::Skip => {}
                ReferrerStep::Stop => break,
                ReferrerStep::Migrate => {
                    migration_guard =
                        Some(self.state.inner.referrer_migration_locks.lock(key.as_str()).await);
                    migrated = Some(read_image_document(storage, key).await?);
                    continue;
                }
                ReferrerStep::Read { unindexed } => {
                    let manifest =
                        self.read_referrer_manifest(storage, key, entry, &mut page).await?;
                    match page.push_referrer(entry, manifest, filter, unindexed) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(response) => return Err(*response),
                    }
                }
            }
            page.cursor = Some(&entry.digest);
            entries.next();
        }
        page.more = entries.peek().is_some();
        Ok(page)
    }

    /// Read and parse one indexed manifest, charging it against the page's
    /// read budget.
    pub(super) async fn read_referrer_manifest(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        entry: &ManifestEntry,
        page: &mut ReferrerPage<'_>,
    ) -> Result<Manifest, Response> {
        let read = read_manifest_bytes(
            storage,
            key,
            &entry.digest.blob_filename(),
            self.state.inner.config.oci.max_manifest_bytes,
        )
        .await;
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                return Err(error(ErrorCode::ManifestUnknown, "a referenced manifest is missing"));
            }
            Err(err) => return Err(registry_error(err)),
        };
        page.charge_read(bytes.len() as u64);
        Manifest::parse(&bytes, Some(&entry.media_type))
            .map_err(|err| registry_error(RegistryError::Internal { reason: err.to_string() }))
    }

    /// Write back the referrer metadata this scan had to read for, so the next
    /// one answers from the index alone.
    pub(super) async fn record_referrer_index(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        additions: &[ManifestEntry],
    ) -> Result<(), Response> {
        if additions.is_empty() {
            return Ok(());
        }
        let _guard = self.state.inner.package_locks.lock(key.as_str()).await;
        storage
            .update_hosted_document_with_retry(key, DOCUMENT_WRITE_RETRIES, |existing| {
                let Some(bytes) = existing else { return Ok(None) };
                let mut current = ImageDocument::parse(bytes)?;
                let mut changed = false;
                for entry in additions {
                    if current.manifest(&entry.digest).is_some_and(|held| held.referrer.is_none()) {
                        current.insert_manifest(entry.clone());
                        changed = true;
                    }
                }
                Ok(changed.then(|| current.to_bytes()))
            })
            .await
            .map_err(registry_error)?;
        Ok(())
    }

    pub(super) fn referrers_page_response(
        &self,
        key: &CanonicalPackageName,
        filter: &ReferrerFilter,
        page: &ReferrerPage<'_>,
    ) -> Response {
        let manifests = page
            .referrers
            .iter()
            .map(|(entry, manifest)| ReferrerDescriptor::new(entry, manifest))
            .collect();
        let mut response = json(
            StatusCode::OK,
            &Referrers { schema_version: 2, media_type: media_type::OCI_IMAGE_INDEX, manifests },
        );
        insert_header(&mut response, "content-type", media_type::OCI_IMAGE_INDEX);
        if filter.artifact_type.is_some() {
            insert_header(&mut response, "oci-filters-applied", "artifactType");
        }
        if let Some(link) = page.next_link(&self.base, key, filter) {
            insert_header(&mut response, "link", &link);
        }
        response
    }
}
