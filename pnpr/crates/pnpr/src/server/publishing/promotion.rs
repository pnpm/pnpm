//! Promoting a publish's staged archives into the hosted store before its
//! metadata commit, and the fence the commit checks them against.
//!
//! Each archive is stored by the identity of its bytes: when the namespace
//! holds those bytes live already, nothing is stored and the commit depends
//! on that incarnation; otherwise the next incarnation is stored, and the
//! commit creates it (see [`pnpr_storage::metadata::BlobRow`]).

use super::{RegistryError, StagedPublish};
use crate::server::AppState;
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    BlobId, BlobSlot, PromotedArchive, Storage,
    metadata::{ArtifactMember, BlobFence, BlobRef, BlobRow, Uncommitted},
};
use std::collections::BTreeMap;

/// The archives a publish promoted before its metadata commit.
#[derive(Default)]
pub(super) struct Promoted {
    /// The archives of other ecosystems than npm, whose publication members
    /// the metadata cannot derive.
    pub(super) artifacts: Vec<ArtifactMember>,
    /// What the commit maps and checks: every archive promoted, and the ones
    /// the staged documents reference.
    pub(super) fence: BlobFence,
    /// The incarnations this publish stored, by namespace.
    stored: Vec<((String, Ecosystem), BlobRef)>,
}

/// Promote every staged archive, and fence the commit with them and the
/// archives the staged documents reference. A failure discards what was
/// stored so far and removes the staged files left.
pub(super) async fn promote_fenced(
    state: &AppState,
    staged: &mut [StagedPublish],
) -> Result<Promoted, RegistryError> {
    let mut promoted = Promoted::default();
    for index in 0..staged.len() {
        let stage = &mut staged[index];
        let namespace = (stage.org.clone().unwrap_or_default(), stage.ecosystem);
        promoted.fence.references.append(&mut stage.base.references);
        let storage = state.inner.storage.for_hosted(&namespace.0, namespace.1);
        let slots = std::mem::take(&mut stage.slots);
        match promote_slots(&storage, slots, &stage.base.blobs).await {
            Ok(archives) => promoted.add(&namespace, archives),
            Err(err) => {
                for stage in &mut staged[index..] {
                    super::cleanup_tmp_slots(std::mem::take(&mut stage.slots)).await;
                }
                promoted.discard(state).await;
                return Err(err);
            }
        }
    }
    Ok(promoted)
}

impl Promoted {
    fn add(&mut self, namespace: &(String, Ecosystem), archives: Vec<PromotedArchive>) {
        for archive in archives {
            if archive.stored {
                self.stored.push((namespace.clone(), archive.artifact.blob.clone()));
            }
            if namespace.1 != Ecosystem::Npm {
                let artifact = &archive.artifact;
                self.artifacts.push(ArtifactMember {
                    ecosystem: artifact.ecosystem,
                    name: artifact.name.clone(),
                    version: String::new(),
                    artifact: artifact.artifact.clone(),
                });
            }
            self.fence.artifacts.push(archive.artifact);
        }
    }

    /// Discard the incarnations this publish stored, after its commit was
    /// refused, so no archive outlives the commit that would have mapped
    /// it. One another publish of the same bytes committed in the meantime
    /// is live and stays; one a commit still on its way could make live is
    /// fenced off first, which refuses that commit. What cannot be removed is
    /// left for `pnpr gc`.
    pub(super) async fn discard(&self, state: &AppState) {
        let mut by_namespace: BTreeMap<&(String, Ecosystem), Vec<Uncommitted>> = BTreeMap::new();
        for (namespace, blob) in &self.stored {
            by_namespace
                .entry(namespace)
                .or_default()
                .push(Uncommitted { blob: blob.clone(), may_fence: true });
        }
        for ((org, ecosystem), stored) in by_namespace {
            let storage = state.inner.storage.for_hosted(org, *ecosystem);
            if let Err(err) = storage.remove_uncommitted(stored).await {
                tracing::warn!(%org, %err, "a refused publish left archives for `pnpr gc`");
            }
        }
    }
}

/// Promote the staged `slots` of one namespace. `known` holds the rows the
/// publish read with its document; the rows of the rest, whose identity was
/// known only once they were written, are read in one more round trip.
async fn promote_slots(
    storage: &Storage,
    slots: Vec<BlobSlot>,
    known: &BTreeMap<BlobId, Option<BlobRow>>,
) -> Result<Vec<PromotedArchive>, RegistryError> {
    let identified = identify_slots(storage, slots).await?;
    let missing = identified
        .iter()
        .map(|(_, (id, _))| id)
        .filter(|id| !known.contains_key(*id))
        .cloned()
        .collect();
    let looked_up = match storage.blob_rows(missing).await {
        Ok(rows) => rows,
        Err(err) => {
            super::cleanup_tmp_slots(
                identified
                    .into_iter()
                    .map(|(slot, _)| slot)
                    .collect(),
            )
            .await;
            return Err(err);
        }
    };
    store_identified(storage, identified, |id| {
        known
            .get(id)
            .copied()
            .flatten()
            .or_else(|| looked_up.get(id).copied())
    })
    .await
}

/// The identity and size of the bytes of each of `slots`. A failure removes
/// every staged file.
async fn identify_slots(
    storage: &Storage,
    slots: Vec<BlobSlot>,
) -> Result<Vec<(BlobSlot, (BlobId, u64))>, RegistryError> {
    let mut identified = Vec::with_capacity(slots.len());
    let mut pending = slots.into_iter();
    while let Some(slot) = pending.next() {
        match storage.identify_blob_slot(&slot).await {
            Ok(identity) => identified.push((slot, identity)),
            Err(err) => {
                let done = identified.into_iter().map(|(slot, _)| slot);
                let left = std::iter::once(slot).chain(pending);
                super::cleanup_tmp_slots(done.chain(left).collect()).await;
                return Err(err);
            }
        }
    }
    Ok(identified)
}

/// Store each identified slot under the incarnation `row_of` its blob's row
/// gives it. A failure removes the staged files left and discards what was
/// stored.
async fn store_identified(
    storage: &Storage,
    identified: Vec<(BlobSlot, (BlobId, u64))>,
    row_of: impl Fn(&BlobId) -> Option<BlobRow>,
) -> Result<Vec<PromotedArchive>, RegistryError> {
    let mut promoted = Vec::with_capacity(identified.len());
    let mut pending = identified.into_iter();
    while let Some((slot, identity)) = pending.next() {
        let row = row_of(&identity.0);
        match storage.promote_blob_slot(slot, identity, row.as_ref()).await {
            Ok(archive) => promoted.push(archive),
            Err(err) => {
                super::cleanup_tmp_slots(pending.map(|(slot, _)| slot).collect()).await;
                let stored = promoted
                    .into_iter()
                    .filter(|archive| archive.stored)
                    .map(|archive| Uncommitted { blob: archive.artifact.blob, may_fence: true })
                    .collect();
                let _ = storage.remove_uncommitted(stored).await;
                return Err(err);
            }
        }
    }
    Ok(promoted)
}
