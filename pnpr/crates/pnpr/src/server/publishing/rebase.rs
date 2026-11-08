//! Rebasing an npm publish onto a packument another replica committed after
//! the publish read its own.
//!
//! A publish computes its packument from the one the store held when it was
//! staged, and commits it only while the store still holds that one. On a
//! single replica the package lock makes that hold. Replicas sharing one
//! metadata database can still race: the publish that commits second finds
//! another generation and is refused. Rather than answer `409` to a publish
//! whose bytes are already in place, the commit takes what the publish
//! changed and applies it to the packument the store now holds, as if the two
//! publishes had run one after the other.

use super::{RegistryError, StagedPublish};
use crate::server::AppState;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::metadata::{
    ArtifactMember, BatchDocument, BlobFence, PublicationCommit, PublicationRequest, PublishNotes,
};
use serde_json::{Map, Value};

/// How many times a publish that lost the race for its packages to another
/// replica is rebased onto what that replica committed and tried again.
const REBASE_ATTEMPTS: usize = 4;

/// The packument each staged npm publish was computed from, taken out of
/// `staged`, for a commit that loses the race to rebase off. `None` unless
/// every document is an npm packument without a client revision precondition;
/// a lost race of any other answers the conflict.
pub(super) fn rebase_bases(staged: &mut [StagedPublish]) -> Option<Vec<Option<Value>>> {
    if staged
        .iter()
        .any(|stage| {
            stage.ecosystem != pnpr_registry::Ecosystem::Npm || stage.base.revision_checked
        })
    {
        return None;
    }
    Some(
        staged
            .iter_mut()
            .map(|stage| stage.base.packument.take())
            .collect(),
    )
}

/// What rebasing a lost commit's documents came to.
enum Rebase {
    /// Every document now applies to what its package holds.
    Applied,
    /// One of them can't be rebased, and the conflict stands.
    Conflict,
}

/// Rebase every document onto the packument its package now holds, and
/// expect that packument's generation.
async fn rebase_documents(
    state: &AppState,
    catalog: &str,
    documents: &mut [BatchDocument],
    rebases: (&[Option<Value>], &[Value]),
) -> Result<Rebase, RegistryError> {
    let storage = state.inner.storage.for_hosted(catalog, pnpr_registry::Ecosystem::Npm);
    for ((document, base), ours) in documents
        .iter_mut()
        .zip(rebases.0)
        .zip(rebases.1)
    {
        let name = CanonicalPackageName::from_stored(&document.name, pnpr_registry::Ecosystem::Npm);
        let current = storage.read_hosted_document_for_update(&name).await?;
        match (current, base) {
            (Some(current), _) => {
                let now: Value = serde_json::from_slice(&current.bytes)?;
                let Some(rebased) = rebase_packument(base.as_ref(), ours, &now) else {
                    return Ok(Rebase::Conflict);
                };
                document.bytes = serde_json::to_vec(&rebased)?;
                document.expected = Some(current.version);
            }
            // The package the publish read is gone: unpublishing it may have
            // left a tombstone the publish was never checked against.
            (None, Some(_)) => return Ok(Rebase::Conflict),
            (None, None) => {
                document.bytes = serde_json::to_vec(ours)?;
                document.expected = None;
            }
        }
    }
    Ok(Rebase::Applied)
}

/// What a publish commits beside its documents.
pub(super) struct Publication<'catalog> {
    pub(super) catalog: &'catalog str,
    pub(super) artifacts: Vec<ArtifactMember>,
    pub(super) notes: PublishNotes,
    pub(super) request: Option<PublicationRequest>,
    /// The files the commit maps, and the blobs it depends on.
    pub(super) fence: BlobFence,
}

/// Commit `documents`, and when another replica committed one of their
/// packages since they were computed, rebase them onto it off `bases` and
/// try again, a bounded number of times.
pub(super) async fn commit_rebasing(
    state: &AppState,
    publication: Publication<'_>,
    mut documents: Vec<BatchDocument>,
    bases: Option<Vec<Option<Value>>>,
) -> Result<PublicationCommit, RegistryError> {
    let catalog = publication.catalog;
    let database = state.inner.storage.sql_metadata();
    // What the publish computed, parsed only once a race calls for it.
    let mut ours: Option<Vec<Value>> = None;
    let mut attempts = 0;
    loop {
        let commit = database.commit_fenced_publication(
            catalog,
            documents.clone(),
            publication.artifacts.clone(),
            publication.notes.clone(),
            publication.request.clone(),
            publication.fence.clone(),
        );
        let conflict = match (commit.await, &bases) {
            (Err(conflict @ RegistryError::DocumentWriteConflict { .. }), Some(bases))
                if attempts < REBASE_ATTEMPTS =>
            {
                (conflict, bases)
            }
            (outcome, _) => return outcome,
        };
        attempts += 1;
        let ours = match &mut ours {
            Some(ours) => ours,
            None => ours.insert(
                documents
                    .iter()
                    .map(|document| serde_json::from_slice(&document.bytes))
                    .collect::<Result<_, _>>()?,
            ),
        };
        match rebase_documents(state, catalog, &mut documents, (conflict.1, ours)).await? {
            Rebase::Applied => {}
            Rebase::Conflict => return Err(conflict.0),
        }
    }
}

/// The packument `current` with every change `ours` made to `base` applied,
/// or `None` when its maintainers are not the ones the publish was
/// authorized against: someone else claimed the package, or an owner was
/// added or removed. Inside `versions`, `dist-tags`, and `time` only the keys
/// `ours` changed are taken, so a version, tag, or time another publish added
/// stays; any other field `ours` changed is taken whole. A key both changed
/// takes `ours`, which commits last.
pub(super) fn rebase_packument(
    base: Option<&Value>,
    ours: &Value,
    current: &Value,
) -> Option<Value> {
    if base.unwrap_or(ours).get("maintainers") != current.get("maintainers") {
        return None;
    }
    let empty = Map::new();
    let base = base.and_then(Value::as_object).unwrap_or(&empty);
    let (ours, current) = (ours.as_object()?, current.as_object()?);
    let mut out = current.clone();
    for key in base.keys().chain(ours.keys()) {
        rebase_field(&mut out, key, base.get(key), ours.get(key));
    }
    Some(Value::Object(out))
}

/// Apply to `out` the change from `before` to `mine` of its field `key`.
fn rebase_field(
    out: &mut Map<String, Value>,
    key: &str,
    before: Option<&Value>,
    mine: Option<&Value>,
) {
    if before == mine {
        return;
    }
    let keyed = matches!(key, "versions" | "dist-tags" | "time");
    match (mine, out.get(key)) {
        (Some(Value::Object(after)), Some(Value::Object(now))) if keyed => {
            let before = before
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let merged = rebase_keys(&before, after, now);
            out.insert(key.to_string(), Value::Object(merged));
        }
        (Some(mine), _) => {
            out.insert(key.to_string(), mine.clone());
        }
        (None, _) => {
            out.shift_remove(key);
        }
    }
}

/// `now` with every key `after` changed from `before` taken from `after`, and
/// every key `after` dropped removed.
fn rebase_keys(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    now: &Map<String, Value>,
) -> Map<String, Value> {
    let mut out = now.clone();
    for key in before.keys().chain(after.keys()) {
        match (before.get(key), after.get(key)) {
            (old, new) if old == new => {}
            (_, Some(new)) => {
                out.insert(key.clone(), new.clone());
            }
            (Some(_), None) => {
                out.shift_remove(key);
            }
            (None, None) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
