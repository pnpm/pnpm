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
    ArtifactMember, BatchDocument, PublicationCommit, PublicationRequest, PublishNotes,
};
use serde_json::{Map, Value};

/// How many times a publish that lost the race for its packages to another
/// replica is rebased onto what that replica committed and tried again.
const REBASE_ATTEMPTS: usize = 4;

/// What a commit that lost a race rebases each document off: the packument
/// the publish read and the one it computed. `None` unless every document is
/// an npm packument, the one format a rebase understands; a lost race of any
/// other answers the conflict.
pub(super) fn rebase_bases(staged: &[StagedPublish]) -> Option<Vec<(Option<Value>, Value)>> {
    staged
        .iter()
        .map(|stage| {
            if stage.ecosystem != pnpr_registry::Ecosystem::Npm {
                return None;
            }
            let ours = serde_json::from_slice(&stage.document).ok()?;
            Some((stage.base.packument.clone(), ours))
        })
        .collect()
}

/// Rebase every document onto the packument its package now holds, and
/// expect that packument's generation.
async fn rebase_documents(
    state: &AppState,
    catalog: &str,
    documents: &mut [BatchDocument],
    rebases: &[(Option<Value>, Value)],
) -> Result<(), RegistryError> {
    let storage = state.inner.storage.for_hosted(catalog, pnpr_registry::Ecosystem::Npm);
    for (document, (base, ours)) in documents.iter_mut().zip(rebases) {
        let name = CanonicalPackageName::from_stored(&document.name, pnpr_registry::Ecosystem::Npm);
        let Some(current) = storage.read_hosted_document_for_update(&name).await? else {
            document.bytes = serde_json::to_vec(ours)?;
            document.expected = None;
            continue;
        };
        let now: Value = serde_json::from_slice(&current.bytes)?;
        let rebased = rebase_packument(base.as_ref(), ours, &now);
        document.bytes = serde_json::to_vec(&rebased)?;
        document.expected = Some(current.version);
    }
    Ok(())
}

/// What a publish commits beside its documents.
pub(super) struct Publication<'catalog> {
    pub(super) catalog: &'catalog str,
    pub(super) artifacts: Vec<ArtifactMember>,
    pub(super) notes: PublishNotes,
    pub(super) request: Option<PublicationRequest>,
}

/// Commit `documents`, and when another replica committed one of their
/// packages since they were computed, rebase them onto it with `rebases` and
/// try again, a bounded number of times.
pub(super) async fn commit_rebasing(
    state: &AppState,
    publication: Publication<'_>,
    mut documents: Vec<BatchDocument>,
    rebases: Option<Vec<(Option<Value>, Value)>>,
) -> Result<PublicationCommit, RegistryError> {
    let Publication { catalog, artifacts, notes, request } = publication;
    let database = state.inner.storage.sql_metadata();
    let mut attempts = 0;
    loop {
        let commit = database.commit_publication(
            catalog,
            documents.clone(),
            artifacts.clone(),
            notes.clone(),
            request.clone(),
        );
        match (commit.await, &rebases) {
            (Err(RegistryError::DocumentWriteConflict { .. }), Some(rebases))
                if attempts < REBASE_ATTEMPTS =>
            {
                attempts += 1;
                rebase_documents(state, catalog, &mut documents, rebases).await?;
            }
            (outcome, _) => return outcome,
        }
    }
}

/// The packument `current` with every change `ours` made to `base` applied:
/// a top-level field `ours` changed is taken from `ours`, and inside an
/// object field (`versions`, `dist-tags`, `time`) only the keys it changed
/// are, so a version, tag, or time another publish added stays. A key both
/// changed takes `ours`, which commits last.
pub(super) fn rebase_packument(base: Option<&Value>, ours: &Value, current: &Value) -> Value {
    let empty = Map::new();
    let base = base.and_then(Value::as_object).unwrap_or(&empty);
    let (Some(ours), Some(current)) = (ours.as_object(), current.as_object()) else {
        return ours.clone();
    };
    let mut out = current.clone();
    for key in base.keys().chain(ours.keys()) {
        let (theirs_before, mine) = (base.get(key), ours.get(key));
        if theirs_before == mine {
            continue;
        }
        match (theirs_before, mine, out.get(key)) {
            (Some(Value::Object(before)), Some(Value::Object(after)), Some(Value::Object(now))) => {
                let merged = rebase_keys(before, after, now);
                out.insert(key.clone(), Value::Object(merged));
            }
            (None, Some(Value::Object(after)), Some(Value::Object(now))) => {
                let merged = rebase_keys(&empty, after, now);
                out.insert(key.clone(), Value::Object(merged));
            }
            (_, Some(mine), _) => {
                out.insert(key.clone(), mine.clone());
            }
            (_, None, _) => {
                out.shift_remove(key);
            }
        }
    }
    Value::Object(out)
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
