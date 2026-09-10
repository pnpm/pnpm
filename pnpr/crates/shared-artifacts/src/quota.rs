use super::{
    ARTIFACT_LOCK_POLL_INTERVAL, ArtifactUsage, ObjectStore, ObjectStoreExt,
    PUBLICATION_FINISH_RETRIES, PathBuf, PutMode, PutOptions, PutPayload, QUOTA_WRITE_RETRIES,
    QuotaChange, QuotaCoordination, RECLAMATION_WAIT_RETRIES, RegistryError, Result,
    SharedArtifactStore, UpdateVersion, acquire_artifact_lock, expire_stranded_publications,
    finish_outcome, is_write_conflict, quota_counter_underflow, quota_write_retry_delay,
    register_publication, sleep, storage_quota_error,
};
use futures_util::StreamExt as _;

impl SharedArtifactStore {
    /// Settles which publications are still in flight, and writes that down.
    ///
    /// Deciding it inside the pass that goes on to refuse something would leave
    /// the decision unwritten whenever the refusal happens, so a registration
    /// nobody keeps would be re-stamped on every read and outlive every pass.
    pub(super) async fn expire_publications(&self) -> Result<()> {
        self.mutate_usage(|usage| Ok(expire_stranded_publications(usage))).await?;
        Ok(())
    }

    pub(super) async fn begin_publication(&self, publication: &str) -> Result<()> {
        self.expire_publications().await?;
        for _ in 0..RECLAMATION_WAIT_RETRIES {
            if self.try_begin_publication(publication).await? {
                return Ok(());
            }
            sleep(ARTIFACT_LOCK_POLL_INTERVAL).await;
        }
        Err(RegistryError::Internal {
            reason: "shared artifact reclamation did not finish before publication timed out"
                .to_string(),
        })
    }

    /// Register one publication, reporting `false` while a reclamation holds
    /// the usage document. A write that fails but landed anyway counts as
    /// registered, so the caller is not told a registration it now has failed.
    pub(super) async fn try_begin_publication(&self, publication: &str) -> Result<bool> {
        let registered = self.mutate_usage(|usage| register_publication(usage, publication)).await;
        match registered {
            Ok(begun) => Ok(begun),
            Err(error) => {
                if self.load_usage().await?.0.active_publications.contains(publication) {
                    return Ok(true);
                }
                Err(error)
            }
        }
    }

    pub(super) async fn finish_publication(
        &self,
        publication: &str,
        reclamation_needed: bool,
    ) -> Result<()> {
        for attempt in 0..PUBLICATION_FINISH_RETRIES {
            let finished = self
                .mutate_usage(|usage| {
                    // A registration missing here is not a fault: an expiry pass
                    // can write one off. What must not be lost is the request to
                    // reclaim, since the scopes a failed publication claimed come
                    // back only that way.
                    usage.active_publications.remove(publication);
                    usage.active_publication_times.remove(publication);
                    usage.reclamation_needed |= reclamation_needed;
                    Ok(true)
                })
                .await;
            let error = match finished {
                Ok(updated) => return finish_outcome(updated),
                Err(error) => error,
            };
            let Some(retry_error) =
                self.publication_finish_retry_error(publication, reclamation_needed, error).await?
            else {
                return Ok(());
            };
            if attempt + 1 == PUBLICATION_FINISH_RETRIES {
                return Err(retry_error);
            }
            sleep(quota_write_retry_delay(attempt)).await;
        }
        unreachable!("publication finish loop returns on its final attempt")
    }

    /// The error a failed finish should retry on, or `None` when the usage
    /// document already says the work is done.
    ///
    /// Gone, and the reclamation this publication asked for already recorded:
    /// whether this attempt wrote that or an earlier one did, there is nothing
    /// left to do.
    pub(super) async fn publication_finish_retry_error(
        &self,
        publication: &str,
        reclamation_needed: bool,
        error: RegistryError,
    ) -> Result<Option<RegistryError>> {
        let (usage, _) = match self.load_usage().await {
            Ok(usage) => usage,
            Err(read_error) => return Ok(Some(read_error)),
        };
        let settled = !usage.active_publications.contains(publication)
            && (!reclamation_needed || usage.reclamation_needed);
        Ok((!settled).then_some(error))
    }

    pub(super) async fn reserve_quota(&self, owner: &str, added_bytes: u64) -> Result<()> {
        self.change_quota(owner, added_bytes, QuotaChange::Reserve).await
    }

    pub(super) async fn release_uncommitted(
        &self,
        owner: &str,
        reserved_bytes: u64,
        retained_bytes: u64,
    ) -> Result<()> {
        let unused_bytes =
            reserved_bytes.checked_sub(retained_bytes).ok_or_else(quota_counter_underflow)?;
        if unused_bytes != 0 {
            self.change_quota(owner, unused_bytes, QuotaChange::Release).await?;
        }
        Ok(())
    }

    pub(super) async fn change_quota(
        &self,
        owner: &str,
        bytes: u64,
        change: QuotaChange,
    ) -> Result<()> {
        let changed = self
            .mutate_usage(|usage| {
                self.change_usage(usage, owner, bytes, change)?;
                Ok(true)
            })
            .await?;
        if !changed {
            return Err(RegistryError::Internal {
                reason: "shared artifact quota update did not change usage".to_string(),
            });
        }
        Ok(())
    }

    pub(super) async fn mutate_usage(
        &self,
        mutation: impl Fn(&mut ArtifactUsage) -> Result<bool>,
    ) -> Result<bool> {
        match &self.quota {
            QuotaCoordination::Local { lock_path } => {
                self.mutate_usage_under_lock(lock_path.clone(), mutation).await
            }
            QuotaCoordination::Conditional => self.mutate_usage_conditionally(mutation).await,
        }
    }

    /// Update the usage document behind a file lock, which is enough on a
    /// store that has no conditional writes.
    pub(super) async fn mutate_usage_under_lock(
        &self,
        lock_path: PathBuf,
        mutation: impl Fn(&mut ArtifactUsage) -> Result<bool>,
    ) -> Result<bool> {
        let _lock = acquire_artifact_lock(lock_path).await?;
        let (mut usage, _) = self.load_usage().await?;
        if !mutation(&mut usage)? {
            return Ok(false);
        }
        self.write_usage(&usage, PutMode::Overwrite).await?;
        Ok(true)
    }

    /// Update the usage document with a conditional write, retrying whoever
    /// loses the race.
    pub(super) async fn mutate_usage_conditionally(
        &self,
        mutation: impl Fn(&mut ArtifactUsage) -> Result<bool>,
    ) -> Result<bool> {
        for attempt in 0..QUOTA_WRITE_RETRIES {
            let (mut usage, version) = self.load_usage().await?;
            if !mutation(&mut usage)? {
                return Ok(false);
            }
            let mode = version.map_or(PutMode::Create, PutMode::Update);
            match self.write_usage(&usage, mode).await {
                Ok(()) => return Ok(true),
                Err(RegistryError::ObjectStore(error)) if is_write_conflict(&error) => {
                    sleep(quota_write_retry_delay(attempt)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(RegistryError::Internal {
            reason: "shared artifact quota changed too often while updating storage".to_string(),
        })
    }

    pub(super) fn change_usage(
        &self,
        usage: &mut ArtifactUsage,
        owner: &str,
        bytes: u64,
        change: QuotaChange,
    ) -> Result<()> {
        if matches!(change, QuotaChange::Release) {
            usage.global_bytes =
                usage.global_bytes.checked_sub(bytes).ok_or_else(quota_counter_underflow)?;
            let owner_bytes =
                usage.owner_bytes.get_mut(owner).ok_or_else(quota_counter_underflow)?;
            *owner_bytes = owner_bytes.checked_sub(bytes).ok_or_else(quota_counter_underflow)?;
            return Ok(());
        }
        let owner_bytes = usage.owner_bytes.get(owner).copied().unwrap_or(0);
        let next_owner = owner_bytes.checked_add(bytes).ok_or_else(storage_quota_error)?;
        let next_global = usage.global_bytes.checked_add(bytes).ok_or_else(storage_quota_error)?;
        if next_owner > self.owner_limit || next_global > self.global_limit {
            return Err(storage_quota_error());
        }
        usage.global_bytes = next_global;
        usage.owner_bytes.insert(owner.to_string(), next_owner);
        Ok(())
    }

    pub(super) async fn load_usage(&self) -> Result<(ArtifactUsage, Option<UpdateVersion>)> {
        let path = self.object_path(self.quota_object());
        match self.store.get(&path).await {
            Ok(result) => {
                let version = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                let bytes = result.bytes().await?;
                Ok((serde_json::from_slice(&bytes)?, Some(version)))
            }
            Err(object_store::Error::NotFound { .. }) => {
                let mut usage = self.scan_usage().await?;
                usage.reclamation_needed = usage.global_bytes != 0;
                Ok((usage, None))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn scan_usage(&self) -> Result<ArtifactUsage> {
        let mut usage = ArtifactUsage::default();
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            if relative == self.quota_object() || relative.starts_with(".locks/") {
                continue;
            }
            let Some((owner, _)) = relative.split_once('/') else { continue };
            let size = entry.size;
            usage.global_bytes =
                usage.global_bytes.checked_add(size).ok_or_else(storage_quota_error)?;
            let owner_bytes = usage.owner_bytes.entry(owner.to_string()).or_default();
            *owner_bytes = owner_bytes.checked_add(size).ok_or_else(storage_quota_error)?;
        }
        Ok(usage)
    }

    pub(super) async fn write_usage(&self, usage: &ArtifactUsage, mode: PutMode) -> Result<()> {
        self.store
            .put_opts(
                &self.object_path(self.quota_object()),
                PutPayload::from(serde_json::to_vec(usage)?),
                PutOptions { mode, ..PutOptions::default() },
            )
            .await?;
        Ok(())
    }
}
