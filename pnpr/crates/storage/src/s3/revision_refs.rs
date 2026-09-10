use super::{
    HOSTED_REVISION_REF_INDEX_FILE, HOSTED_REVISION_REFS_DIR, HostedRevisionRefIndex,
    HostedRevisionRefWrite, ObjectPath, ObjectStoreExt, PutMode, PutOptions, PutPayload,
    REVISION_REF_WRITE_RETRIES, RegistryError, Result, S3Store, UpdateVersion,
    wait_after_document_write_conflict,
};

impl S3Store {
    pub async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        let Some((index, _)) = self.read_revision_ref_index(digest).await? else {
            return Ok(Vec::new());
        };
        Ok(index.bodies().map(<[u8]>::to_vec).collect())
    }

    pub async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        for attempt in 0..REVISION_REF_WRITE_RETRIES {
            let current = self.read_revision_ref_index(digest).await?;
            let (mut index, version) = match current {
                Some((index, version)) => (index, Some(version)),
                None => (HostedRevisionRefIndex::default(), None),
            };
            let outcome = index.insert(ref_id, owner, bytes)?;
            if outcome != HostedRevisionRefWrite::Claimed {
                return Ok(outcome);
            }
            let mode = version.map_or(PutMode::Create, PutMode::Update);
            if self.put_revision_ref_index(digest, &index, mode).await? {
                return Ok(HostedRevisionRefWrite::Claimed);
            }
            if attempt + 1 < REVISION_REF_WRITE_RETRIES {
                wait_after_document_write_conflict(attempt).await;
            }
        }
        let mut index = self
            .read_revision_ref_index(digest)
            .await?
            .map_or_else(HostedRevisionRefIndex::default, |(index, _)| index);
        let outcome = index.insert(ref_id, owner, bytes)?;
        if outcome != HostedRevisionRefWrite::Claimed {
            return Ok(outcome);
        }
        Err(RegistryError::RevisionReferenceWriteConflict { digest: digest.to_string() })
    }

    pub async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        for attempt in 0..REVISION_REF_WRITE_RETRIES {
            let Some((mut index, version)) = self.read_revision_ref_index(digest).await? else {
                return Ok(());
            };
            if !index.remove_if_owned(ref_id, owner) {
                return Ok(());
            }
            if self.put_revision_ref_index(digest, &index, PutMode::Update(version)).await? {
                return Ok(());
            }
            if attempt + 1 < REVISION_REF_WRITE_RETRIES {
                wait_after_document_write_conflict(attempt).await;
            }
        }
        if self
            .read_revision_ref_index(digest)
            .await?
            .is_none_or(|(index, _)| !index.is_owned_by(ref_id, owner))
        {
            return Ok(());
        }
        Err(RegistryError::RevisionReferenceWriteConflict { digest: digest.to_string() })
    }

    pub async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        for attempt in 0..REVISION_REF_WRITE_RETRIES {
            let Some((mut index, version)) = self.read_revision_ref_index(digest).await? else {
                return Err(RegistryError::Internal {
                    reason: "hosted revision reference is missing during commit".to_string(),
                });
            };
            if !index.commit_if_owned(ref_id, owner)? {
                return Ok(());
            }
            if self.put_revision_ref_index(digest, &index, PutMode::Update(version)).await? {
                return Ok(());
            }
            if attempt + 1 < REVISION_REF_WRITE_RETRIES {
                wait_after_document_write_conflict(attempt).await;
            }
        }
        let Some((mut index, _)) = self.read_revision_ref_index(digest).await? else {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference is missing during commit".to_string(),
            });
        };
        if !index.commit_if_owned(ref_id, owner)? {
            return Ok(());
        }
        Err(RegistryError::RevisionReferenceWriteConflict { digest: digest.to_string() })
    }

    /// Write the revision-reference index back under `mode`, reporting whether
    /// it landed. A lost precondition — the object appeared, vanished, or moved
    /// on since it was read — is a conflict the caller retries, not an error.
    pub(super) async fn put_revision_ref_index(
        &self,
        digest: &str,
        index: &HostedRevisionRefIndex,
        mode: PutMode,
    ) -> Result<bool> {
        match self
            .store
            .put_opts(
                &self.revision_ref_index_key(digest),
                PutPayload::from(index.to_bytes()),
                PutOptions { mode, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn read_revision_ref_index(
        &self,
        digest: &str,
    ) -> Result<Option<(HostedRevisionRefIndex, UpdateVersion)>> {
        match self.store.get(&self.revision_ref_index_key(digest)).await {
            Ok(result) => {
                let version = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                let index = HostedRevisionRefIndex::from_bytes(&result.bytes().await?)?;
                Ok(Some((index, version)))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) fn revision_ref_index_key(&self, digest: &str) -> ObjectPath {
        ObjectPath::from(format!(
            "{}{HOSTED_REVISION_REFS_DIR}/{digest}/{HOSTED_REVISION_REF_INDEX_FILE}",
            self.prefix,
        ))
    }
}
