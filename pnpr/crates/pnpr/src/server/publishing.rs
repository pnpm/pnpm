pub(super) use attachments::{
    StagedPublish, ValidatedPublish, stage_publish, validate_publish_doc,
};

mod attachments;

use std::collections::{BTreeSet, HashSet};

use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpm_crypto_hash::{create_hex_hash, integrity_addressed_tarball_path};
use serde_json::{Value, json};
use ssri::Integrity;

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::{Ecosystem, Registry};
use pnpr_storage::{
    HostedDocumentForUpdate, HostedDocumentVersion, Storage,
    journal::{CommitOutcome, JournaledPublish, JournaledRevisionRef},
    publish::{
        PendingAttachment, extract_attachments, merge_manifest, now_iso,
        stream_decode_verify_and_write,
    },
};

use super::{
    Action, AppState, AuthedCaller, HostedGate, HostedOriginalRef, RegistrySource, WriteTarget,
    authorize, authorized_upstream, default_registry_target, documents::RegistryDocuments,
    hosted_gate, hosted_storage, resolve_ecosystem_source, resolve_write_target,
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
    let target = state.inner.config.registries.addressed(registry, ecosystem)?;
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
    let concrete_visible = |name: &str| match state.inner.config.registries.get(name) {
        // The name being probed is unclaimed, so there is no per-package
        // entry to consult: the registry-level default `access:` decides
        // whether the caller may learn the registry exists at all.
        Some(Registry::Hosted { .. }) => state
            .inner
            .config
            .hosted
            .get(name)
            .is_some_and(|hosted| hosted.rules.default_access().allows(identity)),
        Some(Registry::Upstream { .. }) => true,
        Some(Registry::Router { .. }) | None => false,
    };
    match state.inner.config.registries.get(name) {
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

    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };

    // Reject a publish whose body name disagrees with the URL.
    // npm/verdaccio return 400 here too; without this check a
    // misrouted PUT silently overwrites the wrong on-disk
    // package.json with another package's manifest.
    let body_name = incoming.get("name").and_then(Value::as_str);
    if body_name.is_some_and(|body_name| body_name != name.as_str()) {
        return RegistryError::BadRequest {
            reason: format!(
                "package in URL ({:?}) does not match body ({:?})",
                name.as_str(),
                body_name.unwrap_or(""),
            ),
        }
        .into_response();
    }

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
    let _packument_guard = state.inner.package_locks.lock(validated.name.as_str()).await;

    let staged = match stage_publish(state, validated, &now_iso(), Some(&target.org)).await {
        Ok(staged) => staged,
        Err(err) => return err.into_response(),
    };
    match commit_publishes(state, vec![staged]).await.and_then(report_unrecorded) {
        Ok(()) => publish_created_response(),
        Err(err) => err.into_response(),
    }
}

pub(super) async fn serve_batch_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let Value::Object(mut incoming) = incoming else {
        return RegistryError::BadRequest { reason: "body must be a JSON object".to_string() }
            .into_response();
    };
    let Some(Value::Array(docs)) = incoming.remove("packages") else {
        return RegistryError::BadRequest {
            reason: "body must have a `packages` array".to_string(),
        }
        .into_response();
    };
    if docs.is_empty() {
        return RegistryError::BadRequest { reason: "`packages` must not be empty".to_string() }
            .into_response();
    }

    let validated = match validate_batch_docs(&state, &identity, docs).await {
        Ok(validated) => validated,
        Err(err) => return err.into_response(),
    };

    // Hold every affected package's lock across the whole
    // stage-and-commit, so concurrent writers of any package in the
    // batch serialize with us just like with a single publish.
    let names: Vec<&str> = validated.iter().map(|(doc, _)| doc.name.as_str()).collect();
    let _guards = state.inner.package_locks.lock_many(&names).await;

    let staged = match stage_batch(&state, validated).await {
        Ok(staged) => staged,
        Err(err) => return err.into_response(),
    };
    match commit_publishes(&state, staged).await.and_then(report_unrecorded) {
        Ok(()) => publish_created_response(),
        Err(err) => err.into_response(),
    }
}

/// `PUT /-/pnpm/v1/publish` — publish several packages with one
/// request. The body is `{"packages": [<publish doc>, ...]}` where
/// each entry is exactly the JSON body that `PUT /:pkg` takes
/// (packument with `_attachments`). `pnpm publish --batch` sends
/// this; the endpoint is not part of the standard npm registry API.
///
/// The batch is all-or-nothing up to the commit point: every
/// document is validated (name, publish policy, attachment
/// integrity) and every tarball of every package is fully written
/// to a tmp slot before anything becomes visible to readers, so a
/// batch that fails validation or staging leaves no new versions
/// behind.
/// Validate every document of a batch publish, refusing a package named twice.
///
/// One packument read-merge-write happens per package: with the same package
/// twice in a batch, the second entry's merge would depend on the first's
/// uncommitted result. Senders carry multiple versions of one package as
/// several `versions` entries in a single document instead.
///
/// The batch endpoint is path-less, so each package routes via the default
/// target; validation resolves that route and checks the resolved hosted
/// registry's publish rule per document.
async fn validate_batch_docs(
    state: &AppState,
    identity: &Identity,
    docs: Vec<Value>,
) -> Result<Vec<(ValidatedPublish, WriteTarget)>, RegistryError> {
    let mut validated = Vec::with_capacity(docs.len());
    let mut seen_names = std::collections::BTreeSet::new();
    for doc in docs {
        let Some(doc_name) = doc.get("name").and_then(Value::as_str) else {
            return Err(RegistryError::BadRequest {
                reason: "every entry in `packages` must have a string `name`".to_string(),
            });
        };
        let name = CanonicalPackageName::parse(doc_name, pnpr_package_name::Ecosystem::Npm)?;
        if !seen_names.insert(name.as_str().to_string()) {
            return Err(RegistryError::BadRequest {
                reason: format!("duplicate package {:?} in `packages`", name.as_str()),
            });
        }
        validated.push(validate_publish_doc(state, identity, None, name, doc).await?);
    }
    Ok(validated)
}

/// Stage every document of a batch, cleaning up what already landed if one
/// fails.
///
/// Each document's write target was resolved during validation, so a routing
/// failure surfaced before any tarball was staged.
async fn stage_batch(
    state: &AppState,
    validated: Vec<(ValidatedPublish, WriteTarget)>,
) -> Result<Vec<StagedPublish>, RegistryError> {
    let now = now_iso();
    let mut staged: Vec<StagedPublish> = Vec::with_capacity(validated.len());
    for (doc, target) in validated {
        match stage_publish(state, doc, &now, Some(&target.org)).await {
            Ok(stage) => staged.push(stage),
            Err(err) => {
                for stage in staged {
                    cleanup_tmp_slots(stage.slots).await;
                }
                return Err(err);
            }
        }
    }
    Ok(staged)
}

/// Make every staged publish visible, as one journaled transaction: a crash
/// or I/O failure mid-apply can never leave the batch partially published,
/// because startup recovery applies whatever the seal committed. The batch
/// may mix ecosystems; each package's document is merged by the rule its own
/// surface owns.
///
/// A version whose blob lost its immutable slot to another replica is dropped
/// from the document the transaction writes, which keeps the store consistent
/// with the blob that won; the caller reads that out of the outcome. One that
/// could not claim a digest-reference slot is dropped the same way, and
/// reported here: the publisher asked for a version that is not there.
pub(super) async fn commit_publishes(
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
            revision_refs: &stage.revision_refs,
        })
        .collect();
    let outcome = state
        .inner
        .storage
        .publish_journal()
        .commit(&state.inner.storage, &entries, &RegistryDocuments)
        .await?;
    match outcome.reference_limit {
        Some(limit) => Err(RegistryError::RevisionReferenceLimit { limit }),
        None => Ok(outcome),
    }
}

/// The packages a commit could not record, as the error a publisher gets
/// instead of a success it did not earn: another writer owns the blob their
/// version described, so that version is not the one the store serves. Each
/// is named with its ecosystem, since the same name in two of them is two
/// packages.
pub(super) fn report_unrecorded(outcome: CommitOutcome) -> Result<(), RegistryError> {
    let mut missing: BTreeSet<String> = outcome
        .unrecorded
        .into_iter()
        .chain(outcome.lost_blobs.into_iter().map(|lost| lost.package))
        .map(|package| format!("{} {}", package.ecosystem, package.name))
        .collect();
    let Some(first) = missing.pop_first() else {
        return Ok(());
    };
    let packages = std::iter::once(first).chain(missing).collect::<Vec<_>>().join(", ");
    Err(RegistryError::PublishNotRecorded { packages })
}

pub(super) fn publish_created_response() -> Response {
    let body = json!({ "ok": true, "success": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
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
