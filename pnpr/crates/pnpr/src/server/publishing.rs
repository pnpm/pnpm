pub(super) use atomic::{
    commit_publication, publication_request, publish_committed_response, replayed_outcome,
};
pub(super) use attachments::{
    StagedPublish, ValidatedPublish, stage_publish, validate_publish_doc,
};
pub(super) use notes::{check_note_storage, take_note, take_version_note};
pub(super) use tombstones::check_publish_tombstone;

mod atomic;
mod attachments;
mod manifest;
mod notes;
mod tombstones;

use std::collections::{BTreeSet, HashSet};

use axum::response::{IntoResponse, Response};
use serde_json::Value;
use ssri::Integrity;

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::{Ecosystem, Registry};
use pnpr_storage::{
    HostedDocumentVersion, Storage,
    journal::{CommitOutcome, JournaledPublish},
    metadata::PublishNotes,
    publish::{
        PendingAttachment, extract_attachments, now_iso, stream_decode_verify_and_write,
        write_sigstore_bundle,
    },
};

use super::{
    Action, AppState, HostedGate, RegistrySource, WriteTarget, authorize_package,
    authorized_upstream, default_registry_target, documents::RegistryDocuments, hosted_gate,
    hosted_storage, resolve_ecosystem_source, resolve_write_target, restore_tgz_integrities,
    tar_integrity_of, tgz_integrity_key,
};

/// Where a publish of `package` writes, given an optional explicit `/~<name>/`.
pub(super) enum PublishTarget {
    /// Write into the hosted registry `source`'s storage namespace `org`.
    /// The source name is carried so the write's `publish`/`unpublish`
    /// authorization can consult that registry's `packages:` rules.
    Hosted { source: String, org: String },
    /// The resolved target is not a hosted org; reject with this reason.
    Reject(String),
    /// The resolved upstream registry denies this caller; answer with the
    /// same response its reads give (a 403), before any rejection that would
    /// narrate routing config.
    Denied(RegistryError),
    /// The addressed registry or route does not exist (or the path-less base has
    /// no default target).
    NotFound,
}

/// Resolve where a publish lands. A write may only target a hosted registry
/// whose declared patterns claim the name: a selection of an upstream is
/// rejected ("name a hosted registry"), never silently landing on an upstream,
/// and an unclaimed name is rejected with the reason — so a typo'd scope
/// fails loudly at publish time instead of storing a name the registry's
/// namespace can never serve. The registry's `access` list gates the write
/// exactly as it gates reads — a caller the registry denies gets the same
/// not-found mask as on a read, whether the name is claimed or not
/// ([`registry_visible_to_caller`] gates the loud rejection), so a private
/// registry neither accepts the write nor reveals that it exists. The
/// path-less base routes through its default-target registry; with no default
/// target the bare host has no registry and the publish is a not-found,
/// exactly like a read.
pub(super) fn resolve_publish_target_for(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    ecosystem: Ecosystem,
    package: &str,
) -> PublishTarget {
    let Some((target, context)) = addressed_publish_target(state, registry, ecosystem) else {
        return PublishTarget::NotFound;
    };
    match resolve_ecosystem_source(state, &target, ecosystem, package) {
        RegistrySource::Hosted(registry) => {
            match hosted_gate(state, identity, &registry, package) {
                HostedGate::Allowed(org) => PublishTarget::Hosted { source: registry, org },
                HostedGate::MaskNotFound => PublishTarget::NotFound,
                HostedGate::Denied(err) => PublishTarget::Denied(err),
            }
        }
        // A write can never land on an upstream — but the upstream's `access:`
        // gates the write endpoints exactly as it gates reads, so a caller the
        // upstream denies gets the read path's 403 (`authorized_upstream`), not
        // a rejection that narrates where the name routes.
        RegistrySource::Upstream(source) => match authorized_upstream(state, identity, &source) {
            Err(response) => PublishTarget::Denied(response),
            Ok(_) => PublishTarget::Reject(format!(
                "cannot publish {package:?} {context}: it routes to an upstream registry; name \
                 a hosted registry",
            )),
        },
        // The loud rejection explains a config fact about the addressed
        // registry, so only a caller the registry is visible to gets it;
        // anyone else keeps the same not-found mask a read gives, so an
        // off-pattern probe cannot distinguish a private registry from an
        // undefined one.
        RegistrySource::Unclaimed => {
            if registry_visible_to_caller(state, identity, &target) {
                PublishTarget::Reject(format!(
                    "cannot publish {package:?} {context}: no registry's declared `patterns:` \
                     claim this package name",
                ))
            } else {
                PublishTarget::NotFound
            }
        }
        RegistrySource::NotFound => PublishTarget::NotFound,
    }
}

/// The registry a publish addresses, with how to name it in a refusal.
fn addressed_publish_target(
    state: &AppState,
    registry: Option<&str>,
    ecosystem: Ecosystem,
) -> Option<(String, String)> {
    let Some(registry) = registry else {
        let target = default_registry_target(state, ecosystem)?;
        return Some((target, "to the path-less base".to_string()));
    };
    let target = state.inner.config.routing.registries.addressed(registry, ecosystem)?;
    Some((target.to_string(), format!("through registry {registry:?}")))
}

/// Whether `identity` may learn that the registry `name` exists. A hosted
/// registry is masked behind its access list — a denied caller sees the same
/// not-found as for an undefined name on every read, so nothing on the write
/// path may answer differently. An upstream registry is not masked (a denied
/// caller gets an explicit 403 on reads), and a router is visible whenever
/// any of its sources is.
pub(super) fn registry_visible_to_caller(
    state: &AppState,
    identity: &Identity,
    name: &str,
) -> bool {
    let concrete_visible =
        |name: &str| match state.inner.config.routing.registries.get(name) {
            // The name being probed is unclaimed, so there is no per-package
            // entry to consult: the registry-level default `access:` decides
            // whether the caller may learn the registry exists at all.
            Some(Registry::Hosted { .. }) => state.inner.config.routing.hosted
                .get(name)
                .is_some_and(|hosted| hosted.rules.default_access_admits(identity)),
            Some(Registry::Upstream { .. }) => true,
            Some(Registry::Router { .. }) | None => false,
        };
    match state.inner.config.routing.registries.get(name) {
        Some(Registry::Router { sources }) => sources.iter().any(|source| concrete_visible(source)),
        Some(_) => concrete_visible(name),
        None => false,
    }
}

/// `PUT /:pkg` (path-less) or `PUT /~<name>/:pkg` — publish a new version (or
/// republish). Body is the full packument with `_attachments` carrying the
/// tarball bytes base64-encoded.
pub(super) async fn publish_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    body: axum::body::Bytes,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };

    let mut incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    if let Err(err) = check_body_name(&name, &incoming) {
        return err.into_response();
    }

    let request = publication_request(state, identity, &incoming, None, None);
    match replayed_outcome(state, request.as_ref()).await {
        Ok(Some(replayed)) => return replayed,
        Ok(None) => {}
        Err(err) => return err.into_response(),
    }
    let notes = match single_publish_notes(state, &mut incoming) {
        Ok(notes) => notes,
        Err(err) => return err.into_response(),
    };

    // Routing, masking, and the publish rule all run inside
    // `validate_publish_doc`: the write resolves to a hosted registry (or
    // fails closed), and that registry's `packages:` rules authorize it.
    let (validated, target) =
        match validate_publish_doc(state, identity, registry, name, incoming).await {
            Ok(validated) => validated,
            Err(err) => return err.into_response(),
        };

    // Serialize the read-merge-write against other writers of this same
    // package on this instance, so a concurrent publish can't read the
    // same `existing`, merge a different version, and overwrite ours.
    // Held until this function returns, past the packument write below.
    let _packument_guard = state.inner.locks.packages.lock(validated.name.as_str()).await;

    let staged = match stage_publish(state, validated, &now_iso(), Some(&target.org)).await {
        Ok(staged) => staged,
        Err(err) => return err.into_response(),
    };
    match commit_publication(state, vec![staged], notes, request).await {
        Ok(receipt) => publish_committed_response(&receipt),
        Err(err) => err.into_response(),
    }
}

/// Reject a publish whose body name disagrees with the URL. npm and verdaccio
/// answer 400 here too; without this check a misrouted `PUT` would overwrite
/// another package's document.
fn check_body_name(name: &CanonicalPackageName, incoming: &Value) -> Result<(), RegistryError> {
    let body_name = incoming.get("name").and_then(Value::as_str);
    if body_name.is_none_or(|body_name| body_name == name.as_str()) {
        return Ok(());
    }
    Err(RegistryError::BadRequest {
        reason: format!(
            "package in URL ({:?}) does not match body ({:?})",
            name.as_str(),
            body_name.unwrap_or(""),
        ),
    })
}

/// The version note at the top of a single npm publish document.
fn single_publish_notes(
    state: &AppState,
    incoming: &mut Value,
) -> Result<PublishNotes, RegistryError> {
    let versions = take_version_note(incoming, Ecosystem::Npm)?.into_iter().collect();
    let notes = PublishNotes { publication: None, versions };
    check_note_storage(state, &notes)?;
    Ok(notes)
}

/// Make every staged publish visible, as one journaled transaction: a crash
/// or I/O failure mid-apply can never leave the batch partially published,
/// because startup recovery applies whatever the seal committed. The batch
/// may mix ecosystems; each package's document is merged by the rule its own
/// surface owns.
///
/// A version whose blob lost its immutable slot to another replica is dropped
/// from the document the transaction writes, which keeps the store consistent
/// with the blob that won; the caller reads that out of the outcome.
pub(super) async fn commit_publishes(
    state: &AppState,
    staged: Vec<StagedPublish>,
) -> Result<CommitOutcome, RegistryError> {
    if state.inner.storage.sqlite_metadata().is_some() {
        commit_publication(state, staged, PublishNotes::default(), None).await?;
        return Ok(CommitOutcome::default());
    }
    commit_through_journal(state, staged).await
}

/// [`commit_publishes`] through the publish journal.
async fn commit_through_journal(
    state: &AppState,
    staged: Vec<StagedPublish>,
) -> Result<CommitOutcome, RegistryError> {
    let entries: Vec<JournaledPublish<'_>> = staged
        .iter()
        .map(|stage| JournaledPublish {
            name: &stage.name,
            org: stage.org.as_deref(),
            ecosystem: stage.ecosystem,
            document: &stage.document,
            base_version: stage.base_version.as_ref(),
            slots: &stage.slots,
        })
        .collect();
    state.inner.storage
        .publish_journal()
        .commit(&state.inner.storage, &entries, &RegistryDocuments)
        .await
}

/// The packages a commit could not record, as the error a publisher gets
/// instead of a success it did not earn: another writer owns the blob their
/// version described, so that version is not the one the store serves. Each
/// is named with its ecosystem, since the same name in two of them is two
/// packages.
pub(super) fn report_unrecorded(outcome: CommitOutcome) -> Result<(), RegistryError> {
    let mut missing: BTreeSet<String> = outcome.unrecorded
        .into_iter()
        .chain(outcome.lost_blobs.into_iter().map(|lost| lost.package))
        .map(|package| format!("{} {}", package.ecosystem, package.name))
        .collect();
    let Some(first) = missing.pop_first() else {
        return Ok(());
    };
    let packages = std::iter::once(first)
        .chain(missing)
        .collect::<Vec<_>>()
        .join(", ");
    Err(RegistryError::PublishNotRecorded { packages })
}

/// Remove every tmp tarball file that a partially-completed publish
/// already wrote. Errors are swallowed: the caller is already
/// returning an error response, and a leftover `*.tmp.*` file is
/// harmless beyond a small amount of disk.
pub(super) async fn cleanup_tmp_slots(slots: Vec<pnpr_storage::BlobSlot>) {
    for slot in slots {
        let _ = tokio::fs::remove_file(&slot.tmp_path).await;
    }
}

#[cfg(test)]
mod tests;
