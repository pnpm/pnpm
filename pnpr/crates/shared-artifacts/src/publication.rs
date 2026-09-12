mod recovery;

use super::{
    ACTIVE_PUBLICATION_EXPIRY, ArtifactPayload, BACKFILLED_SCOPE, BTreeMap, BTreeSet,
    CompatibilityScopes, Duration, MAX_RESOLVE_RESPONSE_SIZE, ObjectStoreExt,
    PUBLICATION_RENEWAL_INTERVAL, PreparedPublication, PublicationQuota, PublishArtifactRequest,
    RegistryError, Result, ScopeMarker, SharedArtifactStore, SlotClaim, UNIVERSAL_SCOPE,
    artifact_operation_id, bad_request, blob_id, compatibility_scopes, interval,
    prepare_publication, protocol_error, publication_charge, registered_now, scope_marker_path,
    scope_name, scopes_prefix, verify_stored_blob, verify_upload,
};

impl SharedArtifactStore {
    pub async fn publish(&self, username: &str, request: PublishArtifactRequest) -> Result<bool> {
        let prepared = prepare_publication(username, &request)?;
        let publication = artifact_operation_id()?;
        self.begin_publication(&publication).await?;
        let mut reclamation_needed = false;
        let result = self
            .while_renewing(
                &publication,
                PUBLICATION_RENEWAL_INTERVAL,
                self.publish_active(prepared, &publication, &mut reclamation_needed),
            )
            .await;
        self.complete_publication(&publication, reclamation_needed, result).await
    }

    pub(super) async fn complete_publication<Outcome>(
        &self,
        publication: &str,
        reclamation_needed: bool,
        result: Result<Outcome>,
    ) -> Result<Outcome> {
        let finish = self.finish_publication(publication, reclamation_needed).await;
        if finish.is_ok()
            && let Err(error) = self.try_reclaim_unreferenced_blobs().await
        {
            tracing::warn!(%error, "shared artifact reclamation failed");
        }
        finish?;
        result
    }

    /// Runs `work`, saying at intervals that the publication is still working.
    ///
    /// A registration is written off so that one nobody will remove stops
    /// holding reclamation shut. Renewing keeps that from reaching a
    /// publication that is merely slow: it takes every renewal across the
    /// expiry failing for a live one to go quiet long enough. That is why the
    /// recovery afterwards is not redundant — renewals can fail — but it is
    /// what makes needing it rare rather than ordinary.
    ///
    /// The renewals are a branch of the select rather than a handler around it,
    /// so `work` keeps being polled while a renewal waits. What a renewal waits
    /// for is the lock a local usage mutation holds, and the publication holding
    /// that lock is the one being renewed: handling renewals between polls would
    /// leave each waiting on the other for good.
    pub(super) async fn while_renewing<Outcome>(
        &self,
        publication: &str,
        between_renewals: Duration,
        work: impl Future<Output = Outcome>,
    ) -> Outcome {
        let mut renewals = interval(between_renewals);
        renewals.tick().await;
        let renewing = async {
            loop {
                renewals.tick().await;
                // A renewal that cannot be written is not fatal on its own: the
                // expiry is several renewals wide, and the publication recovers
                // what it lost if it is written off anyway.
                if let Err(error) = self.renew_publication(publication).await {
                    tracing::warn!(%error, "shared artifact publication could not renew");
                }
            }
        };
        tokio::select! {
            outcome = work => outcome,
            () = renewing => unreachable!("renewals stop only when the publication does"),
        }
    }

    pub(super) async fn renew_publication(&self, publication: &str) -> Result<()> {
        self.mutate_usage(|usage| {
            if !usage.active_publications.contains(publication) {
                return Ok(false);
            }
            usage.active_publication_times.insert(publication.to_string(), registered_now());
            Ok(true)
        })
        .await?;
        Ok(())
    }

    pub(super) async fn publish_active(
        &self,
        prepared: PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
    ) -> Result<bool> {
        let (stored, created) =
            self.publish_claimed(prepared, publication, reclamation_needed).await;
        if stored.is_err() && !created.is_empty() {
            // The scopes stay claimed. Giving them back here cannot be ordered
            // against a publication of the same envelope, which recognises these
            // markers rather than creating its own and can store the artifact at
            // any point around the giving back — every arrangement of reading
            // and deleting leaves one interleaving that takes the scopes out
            // from under an artifact that is stored. Reclamation runs only when
            // no publication is in flight, so it can tell a scope no artifact
            // holds from one being claimed right now, and drops it there.
            *reclamation_needed = true;
        }
        stored
    }

    /// Publishes one artifact, reporting the scopes it reserved along with the
    /// outcome so a failure can give them back. The claim comes after the quota
    /// is reserved: a marker is an object like any other, and an owner over
    /// quota must not be able to write one.
    pub(super) async fn publish_claimed(
        &self,
        prepared: PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
    ) -> (Result<bool>, Vec<String>) {
        let mut created = Vec::new();
        let stored =
            self.publish_reserving(prepared, publication, reclamation_needed, &mut created).await;
        (stored, created)
    }

    /// Claim the scopes this publication reaches, reporting the bytes its own
    /// markers keep. `None` means the artifact is already published under
    /// exactly these scopes and there is nothing left to do.
    pub(super) async fn claim_publication_scopes(
        &self,
        prepared: &PreparedPublication,
        created: &mut Vec<String>,
        owner: &str,
        added_bytes: u64,
        reclamation_needed: &mut bool,
    ) -> Result<Option<u64>> {
        let claim = match self.claim_scopes(prepared, created).await {
            Ok(claim) => claim,
            Err(error) => {
                *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
                self.release_uncommitted(owner, added_bytes, 0).await?;
                return Err(error);
            }
        };
        match claim {
            SlotClaim::Held => {
                self.release_uncommitted(owner, added_bytes, 0).await?;
                Ok(None)
            }
            SlotClaim::HeldByAnother => {
                self.release_uncommitted(owner, added_bytes, 0).await?;
                Err(RegistryError::ArtifactAlreadyPublished {
                    owner: owner.to_string(),
                    entry: prepared.entry.clone(),
                })
            }
            // The markers this publication wrote, not the scopes it reaches: one
            // it found already its own was charged to whoever wrote it. They are
            // kept whatever becomes of the artifact, since only reclamation
            // gives a scope back.
            SlotClaim::Free => {
                Ok(Some((created.len() as u64) * prepared.envelope_digest.len() as u64))
            }
        }
    }

    pub(super) async fn publish_reserving(
        &self,
        prepared: PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        // Before reserving anything: a publication of an artifact already stored
        // and already holding its scopes writes nothing, so charging it for what
        // it will not store would refuse a retry an owner at their limit is
        // entitled to. Reads only, so nothing is written ahead of the quota.
        if self.publication_is_complete(&prepared).await? {
            return Ok(false);
        }
        let started = prepared.started;
        let mut prepared = prepared;
        let envelope_size = prepared.envelope_bytes.len() as u64;
        let owner = prepared.owner.clone();

        let new_blobs = self.resolve_new_blobs(&mut prepared, &owner).await?;

        let added_bytes = publication_charge(&prepared, &new_blobs, envelope_size)?;
        if let Err(error) = self.reserve_quota(&owner, added_bytes).await {
            *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }

        let Some(retained_bytes) = self
            .claim_publication_scopes(&prepared, created, &owner, added_bytes, reclamation_needed)
            .await?
        else {
            return Ok(false);
        };
        let mut charge =
            PublicationQuota { owner: &owner, added_bytes, retained_bytes, reclamation_needed };
        self.store_new_blobs(new_blobs, &mut charge).await?;
        let created = self
            .store_envelope(
                &prepared.variant_path,
                prepared.envelope_bytes.clone(),
                envelope_size,
                &mut charge,
            )
            .await?;
        if !self
            .settle_envelope(created, &prepared.variant_path, &prepared.envelope_bytes, charge)
            .await?
        {
            return Err(RegistryError::ArtifactAlreadyPublished { owner, entry: prepared.entry });
        }
        if created && started.elapsed() >= ACTIVE_PUBLICATION_EXPIRY {
            self.recover_expired_publication(&prepared, publication, reclamation_needed).await?;
        }

        Ok(created)
    }

    /// Check every blob the signed manifest names against what is uploaded and
    /// what is already stored, and return the ones this publication must write.
    pub(super) async fn resolve_new_blobs(
        &self,
        prepared: &mut PreparedPublication,
        owner: &str,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let required: BTreeMap<String, u64> = prepared
            .payload
            .manifest
            .added
            .iter()
            .map(|file| (file.integrity.clone(), file.size))
            .collect();
        let mut new_blobs = Vec::new();
        for (integrity, size) in required {
            let integrity: &str = &integrity;
            let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
            let path = format!("{owner}/blobs/{id}");
            let upload = prepared.uploads.remove(integrity);
            verify_upload(&id, integrity, size, upload.as_deref())?;
            let Some(stored) = self.read_object_bounded(&path, size).await? else {
                let Some(bytes) = upload else {
                    return Err(bad_request(format!(
                        "signed manifest references blob {id} without uploading it",
                    )));
                };
                new_blobs.push((path, bytes));
                continue;
            };
            verify_stored_blob(&id, integrity, size, &stored)?;
        }
        Ok(new_blobs)
    }

    /// Store the blobs this publication brings, charging the quota for the ones
    /// it wrote. A store failure gives the reservation back before propagating.
    pub(super) async fn store_new_blobs(
        &self,
        new_blobs: Vec<(String, Vec<u8>)>,
        charge: &mut PublicationQuota<'_>,
    ) -> Result<()> {
        for (path, bytes) in new_blobs {
            let size = bytes.len() as u64;
            match self.create_object(&path, bytes).await {
                Ok(true) => charge.retained_bytes += size,
                Ok(false) => {}
                Err(error) => {
                    charge.retained_bytes += size;
                    self.release_after_store_failure(charge).await?;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    /// Store the artifact envelope, reporting whether this publication is the
    /// one that created it.
    pub(super) async fn store_envelope(
        &self,
        variant_path: &str,
        envelope_bytes: Vec<u8>,
        envelope_size: u64,
        charge: &mut PublicationQuota<'_>,
    ) -> Result<bool> {
        match self.create_object(variant_path, envelope_bytes).await {
            Ok(created) => {
                if created {
                    charge.retained_bytes += envelope_size;
                }
                Ok(created)
            }
            Err(error) => {
                charge.retained_bytes += envelope_size;
                self.release_after_store_failure(charge).await?;
                Err(error)
            }
        }
    }

    /// Release what this publication did not keep, and report whether the
    /// stored envelope is its own.
    ///
    /// Two publications can both find the slot empty, so losing the create is
    /// not by itself an idempotent retry: whoever won may have stored something
    /// else, and reporting success would tell a publisher its artifact is the
    /// one being served when it is not.
    ///
    /// The winner is read before the quota is released and inspected after, so
    /// that a store error here cannot return while this publication is still
    /// charged for an envelope it did not store — a leak that would accumulate
    /// silently and eventually refuse publications that fit.
    pub(super) async fn settle_envelope(
        &self,
        created: bool,
        variant_path: &str,
        envelope_bytes: &[u8],
        charge: PublicationQuota<'_>,
    ) -> Result<bool> {
        let winner = if created {
            Ok(None)
        } else {
            self.read_object_bounded(variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64).await
        };
        let released =
            self.release_uncommitted(charge.owner, charge.added_bytes, charge.retained_bytes).await;
        if let Err(error) = released {
            *charge.reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }
        let winner = match winner {
            Ok(winner) => winner,
            Err(error) => {
                *charge.reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
                return Err(error);
            }
        };
        Ok(created || winner.is_some_and(|winner| winner == envelope_bytes))
    }

    /// Give back the part of the reservation a failed store did not use, and
    /// mark the owner's usage for recovery.
    pub(super) async fn release_after_store_failure(
        &self,
        charge: &mut PublicationQuota<'_>,
    ) -> Result<()> {
        *charge.reclamation_needed = true;
        self.release_uncommitted(charge.owner, charge.added_bytes, charge.retained_bytes).await
    }
}
