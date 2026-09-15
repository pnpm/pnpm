use super::{
    ArtifactPayload, BACKFILLED_SCOPE, BTreeSet, CompatibilityScopes, ObjectStoreExt,
    PreparedPublication, RegistryError, Result, ScopeMarker, SharedArtifactStore, UNIVERSAL_SCOPE,
    blob_id, compatibility_scopes, protocol_error, scope_marker_path, scope_name, scopes_prefix,
    verify_stored_blob,
};
use futures_util::StreamExt as _;

impl SharedArtifactStore {
    /// Re-register before recovery to exclude reclamation, which may have
    /// written off this publication and reclaimed its blobs or scopes.
    pub(in super::super) async fn recover_expired_publication(
        &self,
        prepared: &PreparedPublication,
        publication: &str,
        reclamation_needed: &mut bool,
    ) -> Result<()> {
        *reclamation_needed = true;
        // Registered again first, and only then does the recovery look.
        // Being written off is what let a collector run beside this
        // publication; registering again waits for one that is running and
        // keeps another from starting, so what the recovery reads is still
        // there when it returns. A check on its own could not do that.
        if let Err(error) = self.begin_publication(publication).await {
            // Without the registration nothing can be read and believed, so
            // the artifact is taken out unlooked-at rather than left to
            // stand for blobs a collector may already have taken. The
            // scopes it holds name an artifact that is no longer there,
            // which is what reclamation collects.
            self.store.delete(&self.object_path(&prepared.variant_path)).await?;
            return Err(error);
        }
        self.recover_after_expiry(
            &prepared.owner,
            &prepared.entry,
            &prepared.variant_path,
            &prepared.payload,
            &prepared.envelope_digest,
        )
        .await?;
        Ok(())
    }

    /// Makes good what a publication that ran long enough to be written off may
    /// have lost while it was running.
    ///
    /// Being written off lets reclamation run beside it, and reclamation
    /// collects what nothing references — which, before this publication stored
    /// its envelope, includes the blobs it had uploaded. An envelope naming
    /// files that are not there is worse than no artifact, so they are read back
    /// before the scopes are, and the artifact is taken out if any is gone.
    ///
    /// A scope may have gone to an artifact published while this one was
    /// written off, and that artifact holds it. This one then has no claim on a
    /// machine it reaches, so it takes its own artifact back out rather than
    /// leaving two that reach it — the variant is named for constraints only an
    /// identical artifact shares, so removing it removes nothing else's.
    pub(in super::super) async fn recover_after_expiry(
        &self,
        owner: &str,
        entry: &str,
        variant_path: &str,
        payload: &ArtifactPayload,
        holder: &str,
    ) -> Result<()> {
        self.verify_stored_files(owner, variant_path, payload).await?;
        let scopes = match compatibility_scopes(&payload.compatibility) {
            CompatibilityScopes::Every => BTreeSet::from([UNIVERSAL_SCOPE.to_string()]),
            CompatibilityScopes::These(scopes) => scopes,
        };
        let mut retaken = Vec::new();
        let held = self.retake_scopes(owner, entry, holder, &scopes, &mut retaken).await?
            && self.other_vocabulary_is_free(owner, entry, holder, payload).await?;
        if held {
            return Ok(());
        }
        // The artifact goes first, and the scopes it retook after it. A store
        // error between the two leaves markers held for an artifact that is not
        // there, which refuses artifacts reaching those machines until
        // reclamation drops them; the other order would leave the artifact
        // resolvable while holding nothing, and one reaching the same machines
        // could be published beside it.
        self.store.delete(&self.object_path(variant_path)).await?;
        self.release_retaken_scopes(owner, entry, holder, &retaken).await?;
        Err(RegistryError::ArtifactAlreadyPublished {
            owner: owner.to_string(),
            entry: entry.to_string(),
        })
    }

    /// Every file the envelope names must still be stored. Reclamation running
    /// beside a written-off publication can have collected one, and an envelope
    /// naming files that are not there is worse than no artifact, so the
    /// artifact is taken out when any is gone.
    pub(in super::super) async fn verify_stored_files(
        &self,
        owner: &str,
        variant_path: &str,
        payload: &ArtifactPayload,
    ) -> Result<()> {
        for file in &payload.manifest.added {
            let id = blob_id(&file.integrity).map_err(|err| protocol_error(&err))?;
            let path = format!("{owner}/blobs/{id}");
            let Some(bytes) = self.read_object_bounded(&path, file.size).await? else {
                self.store.delete(&self.object_path(variant_path)).await?;
                return Err(RegistryError::Internal {
                    reason: format!(
                        "blob {id} of a shared artifact was collected while the publication \
                         storing it was still running",
                    ),
                });
            };
            verify_stored_blob(&id, &file.integrity, file.size, &bytes)?;
        }
        Ok(())
    }

    /// Claim each scope again, recording the ones this publication retook.
    /// Reports whether it still holds every one of them.
    pub(in super::super) async fn retake_scopes(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        scopes: &BTreeSet<String>,
        retaken: &mut Vec<String>,
    ) -> Result<bool> {
        for scope in scopes {
            let path = scope_marker_path(owner, entry, scope);
            if self.create_object(&path, holder.to_string()).await? {
                retaken.push(scope.clone());
                continue;
            }
            if self.scope_marker(owner, entry, scope, holder).await? != ScopeMarker::Ours {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The other form of the vocabulary reaches these machines too, and a
    /// publication that took one while this was written off holds it under a
    /// key this one never claims.
    pub(in super::super) async fn other_vocabulary_is_free(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        payload: &ArtifactPayload,
    ) -> Result<bool> {
        match compatibility_scopes(&payload.compatibility) {
            CompatibilityScopes::Every => self.tagged_scopes_are_free(owner, entry).await,
            CompatibilityScopes::These(_) => {
                Ok(self.scope_marker(owner, entry, UNIVERSAL_SCOPE, holder).await?
                    != ScopeMarker::Another)
            }
        }
    }

    /// Give back the scopes this publication retook before losing the artifact.
    pub(in super::super) async fn release_retaken_scopes(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        retaken: &[String],
    ) -> Result<()> {
        for scope in retaken {
            // Only while it still names this artifact: a marker retaken here can
            // be collected and taken by somebody else before this loop reaches
            // it, and removing it by path alone would take that publication's
            // claim instead.
            if self.scope_marker(owner, entry, scope, holder).await? != ScopeMarker::Ours {
                continue;
            }
            let path = self.object_path(&scope_marker_path(owner, entry, scope));
            match self.store.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Whether nothing holds a scope named by a tag, which is what an artifact
    /// reaching every machine has to know: its own key says nothing about the
    /// keys tagged artifacts take. Stops at the first, and reads no artifact.
    pub(in super::super) async fn tagged_scopes_are_free(
        &self,
        owner: &str,
        entry: &str,
    ) -> Result<bool> {
        let prefix = self.object_path(&scopes_prefix(owner, entry));
        let mut listing = self.store.list(Some(&prefix));
        while let Some(marker) = listing.next().await {
            if scope_name(&marker?.location)
                .is_some_and(|scope| !matches!(scope, UNIVERSAL_SCOPE | BACKFILLED_SCOPE))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
