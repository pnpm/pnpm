//! Promoting a publish's staged archives into the hosted store before its
//! metadata commit, and the fence the commit checks them against.

use super::{RegistryError, StagedPublish};
use crate::server::AppState;
use futures_util::StreamExt as _;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{
    BlobFinalize, BlobSlot, Storage,
    metadata::{ArchiveFence, ArchiveRef, ArtifactMember},
};
use std::path::Path;
use tokio::io::AsyncReadExt as _;

/// Promote every staged archive, and fence the commit with them and the
/// archives the staged documents reference. Every one of them was promoted,
/// or checked present, after its package was read, so the earliest catalog
/// generation read fences them all.
pub(super) async fn promote_fenced(
    state: &AppState,
    staged: &mut [StagedPublish],
) -> Result<Promoted, RegistryError> {
    let token = staged
        .iter()
        .map(|stage| stage.base.catalog_generation)
        .min()
        .unwrap_or_default();
    let references = referenced_archives(staged);
    let mut promoted = promote_archives(state, staged).await?;
    let mut archives = promoted.depended.clone();
    archives.extend(references);
    promoted.fence = ArchiveFence { token, archives };
    Ok(promoted)
}

/// The archives already in the store that staged publishes reference
/// without promoting them.
fn referenced_archives(staged: &[StagedPublish]) -> Vec<ArchiveRef> {
    staged
        .iter()
        .flat_map(|stage| {
            stage.base.references
                .iter()
                .map(|artifact| ArchiveRef {
                    ecosystem: stage.ecosystem,
                    name: stage.name.as_str().to_string(),
                    artifact: artifact.clone(),
                })
        })
        .collect()
}

/// The archives a publish promoted before its metadata commit.
#[derive(Default)]
pub(super) struct Promoted {
    /// The archives of other ecosystems than npm, whose publication members
    /// the metadata cannot derive.
    pub(super) artifacts: Vec<ArtifactMember>,
    /// Every archive promoted, written or found in place, which the commit
    /// depends on.
    depended: Vec<ArchiveRef>,
    /// What the commit checks: the promoted archives and the ones the staged
    /// documents reference.
    pub(super) fence: ArchiveFence,
    /// The archives this publish created.
    written: Vec<(String, pnpr_registry::Ecosystem, CanonicalPackageName, String)>,
}

impl Promoted {
    /// Reclaim the archives this publish created, after its commit was
    /// refused, so no archive outlives the metadata that would have
    /// referenced it. Another replica may have found one of them in place
    /// with the same bytes and committed it: the reclaim leaves what the
    /// package references, and is fenced against a commit still on its way.
    /// What it cannot remove is left for `pnpr gc`.
    pub(super) async fn remove_written(&self, state: &AppState) {
        let mut packages: Vec<(&str, &CanonicalPackageName, Vec<String>)> = Vec::new();
        for (org, _, name, filename) in &self.written {
            match packages
                .iter_mut()
                .find(|(held_org, held, _)| *held_org == org && *held == name)
            {
                Some((_, _, filenames)) => filenames.push(filename.clone()),
                None => packages.push((org, name, vec![filename.clone()])),
            }
        }
        let manifest_limit = state.inner.config.http.oci.max_manifest_bytes;
        for (org, name, filenames) in packages {
            let reclaimed = crate::archive_gc::reclaim::reclaim_unreferenced(
                &state.inner.storage,
                org,
                name,
                filenames,
                manifest_limit,
            );
            if let Err(err) = reclaimed.await {
                tracing::warn!(package = %name.as_str(), %err, "refused publish left its archives");
            }
        }
    }
}

/// Every staged blob slot with the org, ecosystem and package it belongs to,
/// last first, so popping them promotes in staging order.
fn take_slots(
    staged: &mut [StagedPublish],
) -> Vec<((String, pnpr_registry::Ecosystem, pnpr_package_name::CanonicalPackageName), BlobSlot)> {
    let mut slots: Vec<_> = staged
        .iter_mut()
        .flat_map(|stage| {
            let owner =
                (stage.org.clone().unwrap_or_default(), stage.ecosystem, stage.name.clone());
            std::mem::take(&mut stage.slots)
                .into_iter()
                .map(move |slot| (owner.clone(), slot))
        })
        .collect();
    slots.reverse();
    slots
}

/// Promote every staged archive before the metadata commit that references
/// it. A slot another writer holds with other bytes fails the whole publish,
/// so the commit never records part of it, and the archives this publish
/// already wrote are removed again.
async fn promote_archives(
    state: &AppState,
    staged: &mut [StagedPublish],
) -> Result<Promoted, RegistryError> {
    let mut promoted = Promoted::default();
    let mut pending = take_slots(staged);
    while let Some(((org, ecosystem, name), slot)) = pending.pop() {
        let filename = slot.filename().to_string();
        let tmp_path = slot.tmp_path.clone();
        let storage = state.inner.storage.for_hosted(&org, ecosystem);
        match promote_one(&storage, &name, slot).await {
            Ok(BlobFinalize::Written) => {
                promoted.depended.push(archive_ref(ecosystem, &name, &filename));
                promoted.written.push((org, ecosystem, name.clone(), filename.clone()));
            }
            Ok(BlobFinalize::AlreadyIdentical) => {
                promoted.depended.push(archive_ref(ecosystem, &name, &filename));
            }
            outcome @ (Ok(BlobFinalize::Conflict) | Err(_)) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                super::cleanup_tmp_slots(
                    pending
                        .into_iter()
                        .map(|(_, slot)| slot)
                        .collect(),
                )
                .await;
                promoted.remove_written(state).await;
                outcome?;
                return Err(RegistryError::PublishNotRecorded {
                    packages: format!("{ecosystem} {}", name.as_str()),
                });
            }
        }
        if ecosystem != pnpr_registry::Ecosystem::Npm {
            promoted.artifacts.push(artifact_member(ecosystem, &name, filename));
        }
    }
    Ok(promoted)
}

/// Promote one staged archive, never over an archive already in place: a
/// filesystem store would rename over it and still report it written. An
/// archive in place with the same bytes is already identical; with other
/// bytes it is a conflict. Either way the staged file is left for the
/// caller. `Written` therefore always means this publish created the
/// archive.
async fn promote_one(
    storage: &Storage,
    name: &CanonicalPackageName,
    slot: BlobSlot,
) -> Result<BlobFinalize, RegistryError> {
    let Some((existing, length)) = storage.open_hosted_blob(name, slot.filename()).await? else {
        return storage.finalize_blob_slot(slot).await;
    };
    if !same_bytes(existing, length, &slot.tmp_path).await? {
        return Ok(BlobFinalize::Conflict);
    }
    let _ = tokio::fs::remove_file(&slot.tmp_path).await;
    Ok(BlobFinalize::AlreadyIdentical)
}

/// Bytes compared at a time, so comparing never holds a whole archive.
const COMPARE_CHUNK: usize = 64 * 1024;

/// Whether the archive `existing` streams holds the bytes of the file at
/// `staged`, read a bounded chunk at a time.
pub(super) async fn same_bytes(
    existing: axum::body::Body,
    length: Option<u64>,
    staged: &Path,
) -> Result<bool, RegistryError> {
    let mut file = tokio::fs::File::open(staged).await?;
    let staged_length = file.metadata().await?.len();
    if length.is_some_and(|length| length != staged_length) {
        return Ok(false);
    }
    let mut buffer = vec![0; COMPARE_CHUNK];
    let mut chunks = existing.into_data_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|err| RegistryError::Internal {
            reason: format!("reading an archive: {err}"),
        })?;
        for part in chunk.chunks(COMPARE_CHUNK) {
            let read = &mut buffer[..part.len()];
            if file.read_exact(read).await.is_err() || read != part {
                return Ok(false);
            }
        }
    }
    Ok(file.read(&mut buffer[..1]).await? == 0)
}

fn archive_ref(
    ecosystem: pnpr_registry::Ecosystem,
    name: &CanonicalPackageName,
    artifact: &str,
) -> ArchiveRef {
    ArchiveRef { ecosystem, name: name.as_str().to_string(), artifact: artifact.to_string() }
}

/// A publication member for an archive whose version the metadata does not
/// tie it to.
fn artifact_member(
    ecosystem: pnpr_registry::Ecosystem,
    name: &CanonicalPackageName,
    artifact: String,
) -> ArtifactMember {
    ArtifactMember { ecosystem, name: name.as_str().to_string(), version: String::new(), artifact }
}
