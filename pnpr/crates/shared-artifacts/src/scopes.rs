use super::{
    BACKFILLED_SCOPE, BTreeSet, CompatibilityScopes, MAX_RESOLVE_RESPONSE_SIZE,
    MAX_SCOPE_MARKER_BYTES, PreparedPublication, Result, ScopeMarker, SharedArtifactStore,
    SignedArtifactEnvelope, SlotClaim, UNIVERSAL_SCOPE, compatibility_scopes, is_variant_file,
    object_name, scope_marker_path,
};
use futures_util::StreamExt as _;

impl SharedArtifactStore {
    /// Reserves every scope this artifact reaches, so no other artifact
    /// reaching any of them can be published beside it.
    ///
    /// Each scope is its own conditional create, and that is what orders two
    /// publications whose constraints merely overlap: they contend on the scope
    /// they share, rather than on a path that only identical constraints agree
    /// on. The work is proportional to this artifact's own tags — 64 at the
    /// most, one in practice — not to what the entry already holds, save for the
    /// single backfill an entry written before markers existed needs.
    pub(super) async fn claim_scopes(
        &self,
        publication: &PreparedPublication,
        created: &mut Vec<String>,
    ) -> Result<SlotClaim> {
        let PreparedPublication {
            owner,
            entry,
            envelope_digest,
            variant_path,
            envelope_bytes,
            payload,
            ..
        } = publication;
        // The markers an entry needed are objects this publication wrote, and
        // they outlive it, so it carries them even though they name artifacts
        // somebody else stored.
        self.backfill_scopes(publication).await?;
        let claimed = match compatibility_scopes(&payload.compatibility) {
            CompatibilityScopes::Every => {
                self.claim_universal_scope(owner, entry, envelope_digest, created).await
            }
            CompatibilityScopes::These(scopes) => {
                self.claim_tagged_scopes(owner, entry, envelope_digest, &scopes, created).await
            }
        };
        let claimed = match claimed {
            Ok(claimed) => claimed,
            Err(error) => return Err(error),
        };
        if !claimed {
            return Ok(SlotClaim::HeldByAnother);
        }
        // The scopes belong to this artifact either way. Whether *this* envelope
        // is the one already stored for them is the variant's own question, and
        // a stored one under a different envelope means two builds share a slot.
        match self.read_object_bounded(variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64).await {
            Ok(Some(stored)) if &stored == envelope_bytes => Ok(SlotClaim::Held),
            Ok(Some(_)) => Ok(SlotClaim::HeldByAnother),
            Ok(None) => Ok(SlotClaim::Free),
            Err(error) => Err(error),
        }
    }

    /// Whether this artifact is stored and already holds every scope it reaches,
    /// which is what a retry of a publication that finished looks like.
    ///
    /// The variant is read first, so a publication of something not yet stored
    /// pays one read to find that out and stops.
    pub(super) async fn publication_is_complete(
        &self,
        publication: &PreparedPublication,
    ) -> Result<bool> {
        let PreparedPublication {
            owner, entry, envelope_digest, variant_path, envelope_bytes, ..
        } = publication;
        if self
            .read_object_bounded(variant_path, MAX_RESOLVE_RESPONSE_SIZE as u64)
            .await?
            .is_none_or(|stored| &stored != envelope_bytes)
        {
            return Ok(false);
        }
        let scopes = match compatibility_scopes(&publication.payload.compatibility) {
            CompatibilityScopes::Every => BTreeSet::from([UNIVERSAL_SCOPE.to_string()]),
            CompatibilityScopes::These(scopes) => scopes,
        };
        for scope in &scopes {
            if self.scope_marker(owner, entry, scope, envelope_digest).await? != ScopeMarker::Ours {
                return Ok(false);
            }
        }
        // Holding its own scopes is not enough: an entry can hold an artifact
        // reaching the same machines from the other side of the vocabulary, and
        // a retry into one of those is refused like any other publication rather
        // than reported as already published.
        match compatibility_scopes(&publication.payload.compatibility) {
            CompatibilityScopes::Every => {
                if !self.tagged_scopes_are_free(owner, entry).await? {
                    return Ok(false);
                }
            }
            CompatibilityScopes::These(_) => {
                if self.scope_marker(owner, entry, UNIVERSAL_SCOPE, envelope_digest).await?
                    == ScopeMarker::Another
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// A universal artifact reaches every scope, and cannot enumerate them to
    /// claim one by one. It takes the reserved key instead, then looks for a
    /// tagged scope it would have contended with — a listing that stops at the
    /// first one and reads nothing.
    pub(super) async fn claim_universal_scope(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        if !self.claim_scope(owner, entry, UNIVERSAL_SCOPE, holder, created).await? {
            return Ok(false);
        }
        self.tagged_scopes_are_free(owner, entry).await
    }

    pub(super) async fn claim_tagged_scopes(
        &self,
        owner: &str,
        entry: &str,
        holder: &str,
        scopes: &BTreeSet<String>,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        for scope in scopes {
            if !self.claim_scope(owner, entry, scope, holder, created).await? {
                return Ok(false);
            }
        }
        // A universal artifact publishing at the same time claims its own key
        // rather than any of these, so reading it afterwards is what settles
        // which of the two arrived first. Nobody holding it is the ordinary
        // case, not a conflict.
        Ok(self.scope_marker(owner, entry, UNIVERSAL_SCOPE, holder).await? != ScopeMarker::Another)
    }

    /// Whether this artifact holds `scope`, having either created the marker or
    /// found one it had already taken. Recognising its own marker is what keeps
    /// republishing an artifact a retry rather than a conflict with itself.
    pub(super) async fn claim_scope(
        &self,
        owner: &str,
        entry: &str,
        scope: &str,
        holder: &str,
        created: &mut Vec<String>,
    ) -> Result<bool> {
        match self.create_object(&scope_marker_path(owner, entry, scope), holder.to_string()).await
        {
            Ok(true) => {
                created.push(scope.to_string());
                Ok(true)
            }
            // A marker that lost the create and then went is nobody's, this
            // artifact's least of all, so it is refused rather than assumed.
            Ok(false) => {
                Ok(self.scope_marker(owner, entry, scope, holder).await? == ScopeMarker::Ours)
            }
            Err(error) => {
                // The write can reach the store and still report failure, and a
                // marker nobody is tracking would refuse every later artifact
                // for this scope. Claiming it only when it turns out to hold
                // this artifact keeps the release from touching another's.
                if self
                    .scope_marker(owner, entry, scope, holder)
                    .await
                    .is_ok_and(|marker| marker == ScopeMarker::Ours)
                {
                    created.push(scope.to_string());
                }
                Err(error)
            }
        }
    }

    /// Who holds a scope, which absence does not answer on its own: another
    /// publication can release a marker between the create that lost and this
    /// read, and treating what is gone as this artifact's own would store it
    /// reserving nothing.
    pub(super) async fn scope_marker(
        &self,
        owner: &str,
        entry: &str,
        scope: &str,
        holder: &str,
    ) -> Result<ScopeMarker> {
        Ok(
            match self
                .read_object_bounded(
                    &scope_marker_path(owner, entry, scope),
                    MAX_SCOPE_MARKER_BYTES,
                )
                .await?
            {
                None => ScopeMarker::Gone,
                Some(stored) if stored == holder.as_bytes() => ScopeMarker::Ours,
                Some(_) => ScopeMarker::Another,
            },
        )
    }

    /// Gives an entry whose artifacts hold no scopes the markers they reach, so
    /// that reading the markers speaks for everything stored.
    ///
    /// This is the one place that reads what an entry already holds, and a
    /// count cannot bound it the way one bounds a lookup: a variant it skipped
    /// would leave the scope that variant reaches unclaimed, which is the hole
    /// markers close. It runs once — an entry holding any marker is already
    /// described by them — and each read is bounded by the envelope limit.
    pub(super) async fn backfill_scopes(&self, publication: &PreparedPublication) -> Result<()> {
        let PreparedPublication { owner, entry, .. } = publication;
        // The sentinel, not the markers: they are written one at a time and the
        // scan stops at the first store error, so a marker only says some
        // artifact was reached, while the sentinel says every one was.
        let done = scope_marker_path(owner, entry, BACKFILLED_SCOPE);
        if self.read_object_bounded(&done, MAX_SCOPE_MARKER_BYTES).await?.is_some() {
            return Ok(());
        }
        let variants = self.list_variant_locations(owner, entry).await?;
        // Legacy variants can reach a scope another already reached — an overlap
        // the markers are being written to stop. Writing the marker once rather
        // than once per variant keeps a crowded entry from turning one backfill
        // into a reservation and a release for each repeat.
        let mut attempted = BTreeSet::new();
        for location in variants {
            let Some((digest, scopes)) = self.variant_scopes(&location).await? else {
                continue;
            };
            for scope in &scopes {
                if !attempted.insert(scope.clone()) {
                    continue;
                }
                self.backfill_scope_marker(owner, entry, scope, &digest).await?;
            }
        }
        self.create_object(&done, Vec::new()).await?;
        Ok(())
    }

    /// Every stored variant of one entry.
    pub(super) async fn list_variant_locations(
        &self,
        owner: &str,
        entry: &str,
    ) -> Result<Vec<object_store::path::Path>> {
        let prefix = self.object_path(&format!("{owner}/entries/{entry}/"));
        let mut listing = self.store.list(Some(&prefix));
        let mut variants = Vec::new();
        while let Some(variant) = listing.next().await {
            let variant = variant?;
            if is_variant_file(object_name(&variant.location)) {
                variants.push(variant.location);
            }
        }
        Ok(variants)
    }

    /// The digest and scopes one stored variant declares, or `None` when it
    /// cannot be read as an envelope at all.
    pub(super) async fn variant_scopes(
        &self,
        location: &object_store::path::Path,
    ) -> Result<Option<(String, BTreeSet<String>)>> {
        let Some(relative) = self.relative_path(location).map(str::to_string) else {
            return Ok(None);
        };
        let Some(bytes) =
            self.read_object_bounded(&relative, MAX_RESOLVE_RESPONSE_SIZE as u64).await?
        else {
            return Ok(None);
        };
        let Ok(envelope) = serde_json::from_slice::<SignedArtifactEnvelope>(&bytes) else {
            return Ok(None);
        };
        let Ok((payload, _)) = envelope.decode_payload() else { return Ok(None) };
        let Ok(digest) = envelope.digest() else { return Ok(None) };
        let scopes = match compatibility_scopes(&payload.compatibility) {
            CompatibilityScopes::Every => BTreeSet::from([UNIVERSAL_SCOPE.to_string()]),
            CompatibilityScopes::These(scopes) => scopes,
        };
        Ok(Some((digest, scopes)))
    }

    /// Write one backfilled scope marker.
    ///
    /// It is reserved before it is written and kept afterwards, like every
    /// marker: these outlive the publication that writes them, and an owner
    /// over quota must not be able to write one either.
    pub(super) async fn backfill_scope_marker(
        &self,
        owner: &str,
        entry: &str,
        scope: &str,
        digest: &str,
    ) -> Result<()> {
        let bytes = digest.len() as u64;
        self.reserve_quota(owner, bytes).await?;
        match self.create_object(&scope_marker_path(owner, entry, scope), digest.to_string()).await
        {
            Ok(true) => Ok(()),
            Ok(false) => self.release_uncommitted(owner, bytes, 0).await,
            Err(error) => {
                // A store error says nothing about whether the marker landed, so
                // what is charged is settled by looking rather than assumed.
                // Only a marker this write put there stays charged: one that is
                // not there is nobody's to pay for, and one holding another
                // digest is charged to whoever wrote it. A read that fails too
                // leaves the charge standing, since letting storage outgrow a
                // quota is the worse way to be wrong.
                if self.scope_marker(owner, entry, scope, digest).await? != ScopeMarker::Ours {
                    self.release_uncommitted(owner, bytes, 0).await?;
                }
                Err(error)
            }
        }
    }
}
