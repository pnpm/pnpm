use super::{
    ArtifactUsage, BACKFILLED_SCOPE, HashSet, MAX_RESOLVE_RESPONSE_SIZE, MAX_SCOPE_MARKER_BYTES,
    ObjectPath, ObjectStoreExt, RegistryError, Result, SharedArtifactStore, SignedArtifactEnvelope,
    StoredArtifacts, artifact_operation_id, blob_id, digest_segment, entry_owner, is_blob_path,
    is_variant_file, object_name, scope_name,
};
use futures_util::StreamExt as _;

impl SharedArtifactStore {
    pub(super) async fn try_reclaim_unreferenced_blobs(&self) -> Result<()> {
        self.expire_publications().await?;
        let reclamation = artifact_operation_id()?;
        if !self.acquire_reclamation(&reclamation).await? {
            return Ok(());
        }
        let completed = match self.reclaim_unreferenced_blobs().await {
            Ok(usage) => self.complete_reclamation(&reclamation, usage).await,
            Err(error) => Err(error),
        };
        if let Err(error) = completed {
            self.abort_reclamation(&reclamation).await?;
            return Err(error);
        }
        Ok(())
    }

    /// Take the reclamation slot in the usage document. A write that fails but
    /// landed anyway still holds the slot.
    pub(super) async fn acquire_reclamation(&self, reclamation: &str) -> Result<bool> {
        let acquired = self
            .mutate_usage(|usage| {
                // Dropped here rather than merely disregarded, so that the
                // check on completion sees a publication that started during
                // this run rather than one this run decided to ignore.
                if !usage.reclamation_needed
                    || !usage.active_publications.is_empty()
                    || usage.reclamation.is_some()
                {
                    return Ok(false);
                }
                usage.reclamation = Some(reclamation.to_string());
                Ok(true)
            })
            .await;
        match acquired {
            Ok(acquired) => Ok(acquired),
            Err(error) => {
                if self.load_usage().await?.0.reclamation.as_deref() == Some(reclamation) {
                    return Ok(true);
                }
                Err(error)
            }
        }
    }

    pub(super) async fn reclaim_unreferenced_blobs(&self) -> Result<ArtifactUsage> {
        let artifacts = self.referenced_blobs().await?;
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            if is_blob_path(relative) && !artifacts.referenced_blobs.contains(relative) {
                self.store.delete(&entry.location).await?;
                continue;
            }
            // Only when every variant was read: one that was not is stored all
            // the same, and dropping the scopes it holds would let an artifact
            // reaching the same machines be published beside it.
            if artifacts.every_variant_read
                && self.scope_is_abandoned(&entry.location, &artifacts.digests).await?
            {
                self.store.delete(&entry.location).await?;
            }
        }
        self.scan_usage().await
    }

    /// Whether a scope marker names an artifact that was never stored, which is
    /// what a publication that claimed the scope and then failed leaves behind.
    ///
    /// Only reclamation asks: it runs when no publication is in flight, so a
    /// marker with no artifact is abandoned rather than one being claimed at
    /// this moment. A publication cannot tell those apart, which is why it
    /// leaves its own scopes claimed and asks for reclamation instead.
    pub(super) async fn scope_is_abandoned(
        &self,
        location: &ObjectPath,
        stored_artifacts: &HashSet<String>,
    ) -> Result<bool> {
        let Some(scope) = scope_name(location) else { return Ok(false) };
        if scope == BACKFILLED_SCOPE {
            return Ok(false);
        }
        let Some(relative) = self.relative_path(location).map(str::to_string) else {
            return Ok(false);
        };
        let Some(holder) = self.read_object_bounded(&relative, MAX_SCOPE_MARKER_BYTES).await?
        else {
            return Ok(false);
        };
        Ok(String::from_utf8(holder).is_ok_and(|holder| !stored_artifacts.contains(&holder)))
    }

    /// The blobs stored artifacts reference, the artifacts themselves, and
    /// whether every variant was read — all from one pass, since reclamation
    /// asks all three of every envelope it reads.
    ///
    /// A variant it could not read is stored all the same, and its scopes are
    /// its own. Saying so is what keeps a marker it holds from looking
    /// abandoned.
    pub(super) async fn referenced_blobs(&self) -> Result<StoredArtifacts> {
        let mut artifacts = StoredArtifacts::default();
        let mut listing = self.list_objects(None);
        while let Some(entry) = listing.next().await {
            let entry = entry?;
            let Some(relative) = self.relative_path(&entry.location) else { continue };
            let Some(owner) = entry_owner(relative).map(str::to_string) else { continue };
            self.read_stored_artifact(&entry, &owner, &mut artifacts).await?;
        }
        Ok(artifacts)
    }

    /// Fold one stored object into what the reclamation knows: the digest of a
    /// readable variant, and every blob its manifest references.
    ///
    /// An object that cannot be read as an envelope clears
    /// `every_variant_read`, since a blob it references would otherwise look
    /// unreferenced.
    pub(super) async fn read_stored_artifact(
        &self,
        entry: &object_store::ObjectMeta,
        owner: &str,
        artifacts: &mut StoredArtifacts,
    ) -> Result<()> {
        let variant = is_variant_file(object_name(&entry.location));
        if entry.size > MAX_RESOLVE_RESPONSE_SIZE as u64 {
            artifacts.every_variant_read &= !variant;
            return Ok(());
        }
        let Some(bytes) = self.read_object_path(&entry.location).await? else {
            return Ok(());
        };
        let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
            artifacts.every_variant_read &= !variant;
            return Ok(());
        };
        let Ok((payload, _)) = envelope.decode_payload() else {
            artifacts.every_variant_read &= !variant;
            return Ok(());
        };
        if digest_segment(payload.owner.namespace().as_bytes()) != owner {
            return Ok(());
        }
        if variant {
            match envelope.digest() {
                Ok(digest) => {
                    artifacts.digests.insert(digest);
                }
                Err(_) => artifacts.every_variant_read = false,
            }
        }
        for file in payload.manifest.added {
            let Ok(id) = blob_id(&file.integrity) else { continue };
            artifacts.referenced_blobs.insert(format!("{owner}/blobs/{id}"));
        }
        Ok(())
    }

    pub(super) async fn complete_reclamation(
        &self,
        reclamation: &str,
        mut rebuilt: ArtifactUsage,
    ) -> Result<()> {
        rebuilt.reclamation = None;
        rebuilt.reclamation_needed = false;
        let changed = self
            .mutate_usage(|usage| {
                if usage.reclamation.as_deref() != Some(reclamation) {
                    return Err(RegistryError::Internal {
                        reason: "shared artifact reclamation ownership changed".to_string(),
                    });
                }
                if !usage.active_publications.is_empty() {
                    return Err(RegistryError::Internal {
                        reason: "shared artifact publication started during reclamation"
                            .to_string(),
                    });
                }
                *usage = rebuilt.clone();
                Ok(true)
            })
            .await?;
        if !changed {
            return Err(RegistryError::Internal {
                reason: "shared artifact reclamation did not update usage".to_string(),
            });
        }
        Ok(())
    }

    pub(super) async fn abort_reclamation(&self, reclamation: &str) -> Result<()> {
        self.mutate_usage(|usage| {
            if usage.reclamation.as_deref() != Some(reclamation) {
                return Ok(false);
            }
            usage.reclamation = None;
            usage.reclamation_needed = true;
            Ok(true)
        })
        .await?;
        Ok(())
    }
}
