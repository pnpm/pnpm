use super::{
    ApplyContext, ApplyProgress, COMMIT_DOCUMENT_WRITE_RETRIES, CanonicalPackageName,
    CommitOutcome, DocumentMerge, DocumentUpdate, DocumentWrite, HashMap, HashSet, HostedDocuments,
    HostedRevisionRefWrite, JournaledRevisionRef, LostBlob, MANIFEST_FILE, Manifest,
    ManifestPackage, PackageTarget, PathBuf, RegistryError, Result, SealedTxn, Storage,
    cleanup_lost_tmp_paths, fs, promote_blobs, revision_ref_owner, sync_dir,
};
impl SealedTxn {
    /// Reopen a sealed transaction from its journal directory, the way
    /// startup recovery does: with no base versions, so every document is
    /// merged into what the store holds rather than written over it.
    pub(super) fn reopen(dir: PathBuf) -> Result<Self> {
        let revision_ref_owner = revision_ref_owner(&dir)?.to_string();
        Ok(Self {
            dir,
            revision_ref_owner,
            base_versions: Vec::new(),
        })
    }

    /// Run every step of the sealed transaction that has not run yet, then
    /// remove the journal entry. Each step tolerates having already run
    /// before a crash: a tmp file that is gone was already promoted, and the
    /// document is merged into what the store holds rather than overwriting
    /// it, so an interrupted apply just runs again — which is what startup
    /// recovery does.
    pub(super) async fn apply(
        self,
        storage: &Storage,
        documents: &dyn HostedDocuments,
        progress: &mut ApplyProgress,
    ) -> Result<()> {
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(self.dir.join(MANIFEST_FILE)).await?)?;
        let mut context = ApplyContext {
            documents,
            progress,
        };
        let mut lost_tmp_paths = Vec::new();
        for (index, package) in manifest.packages.iter().enumerate() {
            self.apply_package(index, package, storage, &mut context, &mut lost_tmp_paths)
                .await?;
        }
        // Remove the journal before cleaning lost tmp files so an interruption
        // cannot leave a retry that has lost the evidence needed to detect the
        // conflict.
        fs::remove_dir_all(&self.dir).await?;
        // Only clean conflict evidence after the journal removal is durable.
        let journal_removal_is_durable = match self.dir.parent() {
            Some(parent) => sync_dir(parent).await.is_ok(),
            None => false,
        };
        cleanup_lost_tmp_paths(&lost_tmp_paths, journal_removal_is_durable)
            .await;
        Ok(())
    }

    /// Promote one journaled package's blobs, references and document.
    async fn apply_package<'a>(
        &self,
        index: usize,
        package: &'a ManifestPackage,
        storage: &Storage,
        context: &mut ApplyContext<'_>,
        lost_tmp_paths: &mut Vec<&'a std::path::Path>,
    ) -> Result<()> {
        let name = CanonicalPackageName::parse(&package.name, package.ecosystem)?;
        // Promote into the package's hosted namespace (or the flat store
        // when it has none), so the commit and a later startup recovery
        // land in exactly the store the publish targeted.
        let store = match &package.org {
            Some(org) => storage.for_hosted(org),
            None => storage.clone(),
        };
        let target = PackageTarget {
            store,
            name,
        };
        let mut lost_blobs = promote_blobs(&target, package, lost_tmp_paths)
            .await?;
        let claimed = self.claim_revision_refs(
            &target,
            package,
            &mut lost_blobs,
            &mut context.progress.outcome,
        )
        .await?;
        self.write_package_document(&target, package, index, &lost_blobs, context)
            .await?;
        for revision_ref in claimed.into_values().flatten() {
            target.store.commit_hosted_revision_ref(
                &revision_ref.digest,
                &revision_ref.ref_id,
                &self.revision_ref_owner,
            )
            .await?;
        }
        context.progress.outcome.lost_blobs.extend(
            lost_blobs
                .into_iter()
                .map(|filename| LostBlob {
                    package: package.id(),
                    filename,
                }),
        );
        Ok(())
    }

    /// Claim a revision reference for every blob that landed, backing this
    /// package's claims out again if the store's reference limit is reached.
    async fn claim_revision_refs<'a>(
        &self,
        target: &PackageTarget,
        package: &'a ManifestPackage,
        lost_blobs: &mut HashSet<String>,
        outcome: &mut CommitOutcome,
    ) -> Result<HashMap<&'a str, Vec<&'a JournaledRevisionRef>>> {
        let mut claimed: HashMap<&str, Vec<&JournaledRevisionRef>> = HashMap::new();
        for revision_ref in &package.revision_refs {
            if lost_blobs.contains(&revision_ref.filename) {
                continue;
            }
            let write = target.store.write_hosted_revision_ref(
                &revision_ref.digest,
                &revision_ref.ref_id,
                &self.revision_ref_owner,
                &revision_ref.bytes,
            )
            .await;
            match write {
                Ok(HostedRevisionRefWrite::Claimed | HostedRevisionRefWrite::AlreadyClaimed) => {
                    claimed
                        .entry(&revision_ref.filename)
                        .or_default()
                        .push(revision_ref);
                }
                Ok(HostedRevisionRefWrite::Committed) => {}
                Err(RegistryError::RevisionReferenceLimit { limit }) => {
                    outcome.reference_limit = Some(limit);
                    lost_blobs.insert(revision_ref.filename.clone());
                    let backed_out = claimed.remove(revision_ref.filename.as_str());
                    self.release_claims(target, backed_out).await?;
                }
                Err(err) => return Err(err),
            }
        }
        Ok(claimed)
    }

    /// Drop the references already claimed for a blob the limit has cost us.
    async fn release_claims(
        &self,
        target: &PackageTarget,
        claimed: Option<Vec<&JournaledRevisionRef>>,
    ) -> Result<()> {
        for claimed_ref in claimed.into_iter().flatten() {
            target.store.remove_hosted_revision_ref(
                &claimed_ref.digest,
                &claimed_ref.ref_id,
                &self.revision_ref_owner,
            )
            .await?;
        }
        Ok(())
    }

    /// Write the journaled document.
    ///
    /// It was computed from `base_version` of the stored one, so while the
    /// store is still there it is exactly what to write — no second read, no
    /// merge. Recovery carries no base version and always merges.
    async fn write_package_document(
        &self,
        target: &PackageTarget,
        package: &ManifestPackage,
        index: usize,
        lost_blobs: &HashSet<String>,
        context: &mut ApplyContext<'_>,
    ) -> Result<()> {
        let journaled = fs::read(self.dir.join(&package.document_file)).await?;
        if lost_blobs.is_empty()
            && let Some(base_version) = self.base_versions.get(index)
        {
            let write = target.store.write_hosted_document_if_current(
                &target.name,
                &journaled,
                base_version.as_ref(),
            )
            .await?;
            if matches!(write, DocumentWrite::Written) {
                context.progress.wrote_documents.insert(package.id());
                return Ok(());
            }
        }
        let documents = context.documents;
        let update = target.store.update_hosted_document_with_retry(
            &target.name,
            COMMIT_DOCUMENT_WRITE_RETRIES,
            |existing| {
                documents.merge(DocumentMerge {
                    ecosystem: package.ecosystem,
                    name: &target.name,
                    existing,
                    journaled: &journaled,
                    lost_blobs,
                })
            },
        )
        .await?;
        match update {
            DocumentUpdate::Written => {
                context.progress.wrote_documents.insert(package.id());
            }
            DocumentUpdate::NotFound => context.progress.outcome.unrecorded.push(package.id()),
        }
        Ok(())
    }
}
