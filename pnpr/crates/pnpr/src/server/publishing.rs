pub(super) mod variants;

pub(super) use atomic::{
    PublishReceipt, add_npm_notices, commit_publication, publication_name, publication_request,
    publish_committed_response, replayed_outcome, single_catalog,
};
pub(super) use attachments::{
    StagedBase, StagedPublish, ValidatedPublish, Verification, stage_publish, validate_publish_doc,
    validate_staged_publish_name,
};
pub(super) use notes::{refuse_notes_of_published_python_versions, take_note, take_version_note};
pub(super) use tombstones::check_publish_tombstone;

mod atomic;
mod attachments;
mod generated_archive;
mod manifest;
mod notes;
mod promotion;
mod provenance;
mod rebase;
mod release_records;
mod tombstones;
mod verification;

use std::collections::HashSet;

use axum::response::{IntoResponse, Response};
use serde_json::Value;
use ssri::Integrity;

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::{Ecosystem, Registry};
use pnpr_storage::{
    HostedDocumentVersion, Storage,
    metadata::{PublicationRequest, PublishNotes},
    publish::{
        PendingAttachment, extract_attachments, now_iso, stream_decode_verify_and_write,
        verify_and_write, write_sigstore_bundle,
    },
};

use super::{
    Action, AppState, HostedGate, RegistrySource, WriteTarget, authorize_package,
    authorized_upstream, default_registry_target, hosted_gate, hosted_storage,
    resolve_ecosystem_source, resolve_write_target, restore_tgz_integrities,
    staged_archives::StagedReferences, tar_integrity_of, tgz_integrity_key,
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

/// Whether `package` routes to an upstream registry through the registry a
/// request addresses.
pub(super) fn routes_to_upstream(
    state: &AppState,
    registry: Option<&str>,
    ecosystem: Ecosystem,
    package: &str,
) -> bool {
    addressed_publish_target(state, registry, ecosystem)
        .is_some_and(|(target, _)| {
            matches!(
                resolve_ecosystem_source(state, &target, ecosystem, package),
                RegistrySource::Upstream(_),
            )
        })
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

    let request = publication_request(identity, &incoming, None, None, Some(&name));
    let notes = match single_publish_notes(&mut incoming) {
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
    // A retry is authorized like any publish, so only a caller still allowed
    // to make it gets the outcome of the request it repeats. It is looked up
    // under the lock, so a retry that waited for the original sees its commit.
    match replayed_outcome(state, &target.org, request.as_ref()).await {
        Ok(Some(replayed)) => return replayed,
        Ok(None) => {}
        Err(err) => return err.into_response(),
    }

    stage_and_commit(state, identity, validated, &target.org, (notes, request))
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

/// Stage one validated publish into `org` and commit it, then give back the
/// staged archives it claimed.
async fn stage_and_commit(
    state: &AppState,
    identity: &Identity,
    validated: ValidatedPublish,
    org: &str,
    (notes, request): (PublishNotes, Option<PublicationRequest>),
) -> Result<Response, RegistryError> {
    let storage = &state.inner.storage;
    let references =
        StagedReferences::check(storage, identity, validated.staged_integrities()).await?;
    let staged = stage_publish(state, validated, &now_iso(), Some(org)).await?;
    let receipt = commit_publication(state, vec![staged], notes, request).await?;
    references.release(storage).await;
    Ok(publish_committed_response(&receipt))
}

/// Reject a publish whose body name disagrees with the URL. npm and verdaccio
/// answer 400 here too; without this check a misrouted `PUT` would overwrite
/// another package's document.
pub(super) fn check_body_name(
    name: &CanonicalPackageName,
    incoming: &Value,
) -> Result<(), RegistryError> {
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

pub(super) fn check_body_versions(incoming: &Value) -> Result<(), RegistryError> {
    let Some(versions) = incoming.get("versions").and_then(Value::as_object) else {
        return Ok(());
    };
    for (version_key, manifest) in versions {
        if !is_valid_semver(version_key) {
            return Err(RegistryError::BadRequest {
                reason: format!("version key {version_key:?} is not valid semver"),
            });
        }
        let manifest_version = manifest.get("version").and_then(Value::as_str);
        let Some(manifest_version) = manifest_version else {
            return Err(RegistryError::BadRequest {
                reason: format!("version key {version_key:?} has no valid semver manifest version"),
            });
        };
        if !is_valid_semver(manifest_version) {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "manifest version {manifest_version:?} for version key {version_key:?} is not valid semver",
                ),
            });
        }
    }
    Ok(())
}

/// Whether `raw` is a version npm clients can install: strict `SemVer` 2.0,
/// as the `semver` crate parses it for Cargo publishes, within the length and
/// integer limits of the npm parser pnpr reads stored versions with.
fn is_valid_semver(raw: &str) -> bool {
    semver::Version::parse(raw).is_ok() && node_semver::Version::parse(raw).is_ok()
}

/// The version note at the top of a single npm publish document.
fn single_publish_notes(incoming: &mut Value) -> Result<PublishNotes, RegistryError> {
    let versions = take_version_note(incoming, Ecosystem::Npm)?.into_iter().collect();
    Ok(PublishNotes { publication: None, versions })
}

/// Make every staged publish visible in one metadata commit: every archive
/// is promoted first, then every document commits, or none does. The batch
/// may mix ecosystems.
pub(super) async fn commit_publishes(
    state: &AppState,
    staged: Vec<StagedPublish>,
) -> Result<(), RegistryError> {
    commit_publication(state, staged, PublishNotes::default(), None).await?;
    Ok(())
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
