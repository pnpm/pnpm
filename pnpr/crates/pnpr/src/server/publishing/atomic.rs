//! Committing a publish as one metadata transaction, when the metadata lives
//! in `SQLite`: every archive is promoted first, then every document of the
//! request commits under one generation of its organization's catalog, or
//! none does. Readers see the whole request or nothing of it.
//!
//! The request's outcome commits with it. A retry of the same request by the
//! same principal gets that outcome back instead of a conflict, whether or not
//! the client sent an idempotency key: without one, the digest of the whole
//! request is the key.

use super::{RegistryError, StagedPublish, Value, report_unrecorded};
use crate::server::{AppState, Identity};
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use pnpr_storage::{
    BlobFinalize,
    metadata::{ArtifactMember, BatchDocument, Generation, PublicationRequest},
};
use serde_json::{Map, json};

/// What a publish committed, as its response reports it.
#[derive(Debug, Default)]
pub(in super::super) struct PublishReceipt {
    /// The generation holding the publish, when the metadata tracks them.
    pub(in super::super) generation: Option<Generation>,
    /// The publication the commit recorded.
    pub(in super::super) publication: Option<u64>,
}

/// The idempotency claim of a publish request: who sent it, and the request
/// it is a retry of when it is one. `None` for a caller with no stable
/// principal, or a store that keeps no outcomes.
pub(in super::super) fn publication_request(
    state: &AppState,
    identity: &Identity,
    body: &Value,
    key: Option<&str>,
) -> Option<PublicationRequest> {
    state.inner.storage.sqlite_metadata()?;
    let principal = identity.username()?.to_string();
    let request_digest = request_digest(body);
    let idempotency_key = key.map_or_else(|| format!("request:{request_digest}"), str::to_string);
    let name = body
        .get("publication")
        .and_then(|publication| publication.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(PublicationRequest { principal, idempotency_key, request_digest, name })
}

/// The response a retry of an already committed request gets: the outcome
/// that request recorded.
pub(in super::super) async fn replayed_outcome(
    state: &AppState,
    request: Option<&PublicationRequest>,
) -> Result<Option<Response>, RegistryError> {
    let (Some(request), Some(database)) = (request, state.inner.storage.sqlite_metadata()) else {
        return Ok(None);
    };
    let outcome = database.recorded_outcome(
        &request.principal,
        &request.idempotency_key,
        &request.request_digest,
    )
    .await?;
    Ok(outcome.map(|outcome| {
        let receipt = PublishReceipt {
            generation: Some(Generation { catalog: outcome.catalog, value: outcome.generation }),
            publication: outcome.publication,
        };
        publish_committed_response(&receipt)
    }))
}

/// Commit `staged` in one transaction when the metadata lives in `SQLite`,
/// through the publish journal otherwise.
pub(in super::super) async fn commit_publication(
    state: &AppState,
    mut staged: Vec<StagedPublish>,
    request: Option<PublicationRequest>,
) -> Result<PublishReceipt, RegistryError> {
    let Some(database) = state.inner.storage.sqlite_metadata().cloned() else {
        report_unrecorded(super::commit_through_journal(state, staged).await?)?;
        return Ok(PublishReceipt::default());
    };
    let catalog = single_catalog(&staged)?;
    let artifacts = promote_archives(state, &mut staged).await?;
    let documents = staged
        .into_iter()
        .map(|stage| BatchDocument {
            ecosystem: stage.ecosystem,
            name: stage.name.as_str().to_string(),
            bytes: stage.document,
            expected: stage.base_version,
        })
        .collect();
    let commit = database.commit_publication(&catalog, documents, artifacts, request).await?;
    Ok(PublishReceipt { generation: Some(commit.generation), publication: commit.publication })
}

/// The one organization every staged package belongs to. A transaction never
/// spans organizations.
fn single_catalog(staged: &[StagedPublish]) -> Result<String, RegistryError> {
    let mut orgs = staged
        .iter()
        .map(|stage| stage.org.as_deref().unwrap_or_default());
    let first = orgs.next().unwrap_or_default();
    if orgs.any(|org| org != first) {
        return Err(RegistryError::BadRequest {
            reason: "a publish spans more than one organization; publish each one separately"
                .to_string(),
        });
    }
    Ok(first.to_string())
}

/// Promote every staged archive before the metadata commit that references
/// it. A slot another writer holds with other bytes fails the whole publish,
/// so the commit never records part of it. Returns the archives of other
/// ecosystems than npm, whose publication members the metadata cannot
/// derive.
async fn promote_archives(
    state: &AppState,
    staged: &mut [StagedPublish],
) -> Result<Vec<ArtifactMember>, RegistryError> {
    let mut artifacts = Vec::new();
    let mut pending: Vec<_> = staged
        .iter_mut()
        .flat_map(|stage| {
            let owner =
                (stage.org.clone().unwrap_or_default(), stage.ecosystem, stage.name.clone());
            std::mem::take(&mut stage.slots)
                .into_iter()
                .map(move |slot| (owner.clone(), slot))
        })
        .collect();
    pending.reverse();
    while let Some(((org, ecosystem, name), slot)) = pending.pop() {
        let filename = slot.filename().to_string();
        let tmp_path = slot.tmp_path.clone();
        let promoted =
            state.inner.storage.for_hosted(&org, ecosystem).finalize_blob_slot(slot).await;
        if !matches!(promoted, Ok(BlobFinalize::Written | BlobFinalize::AlreadyIdentical)) {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            super::cleanup_tmp_slots(
                pending
                    .into_iter()
                    .map(|(_, slot)| slot)
                    .collect(),
            )
            .await;
            promoted?;
            return Err(RegistryError::PublishNotRecorded {
                packages: format!("{ecosystem} {}", name.as_str()),
            });
        }
        if ecosystem != pnpr_registry::Ecosystem::Npm {
            artifacts.push(artifact_member(ecosystem, &name, filename));
        }
    }
    Ok(artifacts)
}

/// A publication member for an archive whose version the metadata does not
/// tie it to.
fn artifact_member(
    ecosystem: pnpr_registry::Ecosystem,
    name: &pnpr_package_name::CanonicalPackageName,
    artifact: String,
) -> ArtifactMember {
    ArtifactMember { ecosystem, name: name.as_str().to_string(), version: String::new(), artifact }
}

/// The digest of a request, independent of the order of its object keys.
fn request_digest(body: &Value) -> String {
    let canonical = serde_json::to_vec(&canonical(body)).unwrap_or_default();
    ssri::Integrity::from(canonical).to_string()
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut sorted: Vec<_> = fields.iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(right.0));
            let fields: Map<String, Value> = sorted
                .into_iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect();
            Value::Object(fields)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// `201 Created` for a committed publish, naming the generation that holds
/// it and the publication it recorded.
pub(in super::super) fn publish_committed_response(receipt: &PublishReceipt) -> Response {
    let mut body = json!({ "ok": true, "success": true });
    if let Some(publication) = receipt.publication {
        body["publication"] = json!(publication);
    }
    let mut builder = Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(generation) = &receipt.generation {
        builder = builder.header(crate::server::GENERATION_HEADER, generation.to_string());
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("static-shape response always builds")
}
