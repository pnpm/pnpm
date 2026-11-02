//! Committing a publish as one metadata transaction, when the metadata lives
//! in `SQLite`: every archive is promoted first, then every document of the
//! request commits under one generation of its organization's catalog, or
//! none does. Readers see the whole request or nothing of it.
//!
//! The request's outcome commits with it. A retry of the same request by the
//! same principal gets that outcome back instead of a conflict, whether or not
//! the client sent an idempotency key: without one, the digest of the whole
//! request is the key. A retry is authorized against current permissions
//! like any publish before its outcome is looked up.

use super::{RegistryError, StagedPublish, Value, report_unrecorded};
use crate::server::{AppState, Identity};
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use pnpr_storage::{
    BlobFinalize, BlobSlot,
    metadata::{ArtifactMember, BatchDocument, Generation, PublicationRequest},
};
use serde_json::json;
use ssri::{Algorithm, IntegrityOpts};
use std::io::{self, Write as _};

/// What a publish committed, as its response reports it.
#[derive(Debug, Default)]
pub(in super::super) struct PublishReceipt {
    /// The generation holding the publish, when the metadata tracks them.
    pub(in super::super) generation: Option<Generation>,
    /// The publication the commit recorded.
    pub(in super::super) publication: Option<u64>,
}

/// The idempotency claim of a publish request: who sent it, the request it
/// is a retry of when it is one, and the publication's `name`. `None` for a caller with no stable
/// principal, or a store that keeps no outcomes.
pub(in super::super) fn publication_request(
    state: &AppState,
    identity: &Identity,
    body: &Value,
    key: Option<&str>,
    name: Option<&str>,
) -> Option<PublicationRequest> {
    state.inner.storage.sqlite_metadata()?;
    let principal = identity.username()?.to_string();
    let request_digest = request_digest(body);
    let idempotency_key = key.map_or_else(|| format!("request:{request_digest}"), str::to_string);
    let name = name.map(str::to_string);
    Some(PublicationRequest { principal, idempotency_key, request_digest, name })
}

/// The longest publication name, in bytes: the longest npm package name.
const MAX_PUBLICATION_NAME: usize = 214;

/// The publication name a batch body gives in its top-level `name`. A name
/// is never a decimal number, so `/-/publications/<org>/<id-or-name>` reads
/// a segment as one or the other.
pub(in super::super) fn publication_name(body: &Value) -> Result<Option<&str>, RegistryError> {
    // An empty name names nothing.
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty());
    if let Some(name) = name
        && name.len() > MAX_PUBLICATION_NAME
    {
        return Err(RegistryError::BadRequest {
            reason: format!("a publication name is at most {MAX_PUBLICATION_NAME} bytes long"),
        });
    }
    if let Some(name) = name
        && name.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(RegistryError::BadRequest {
            reason: format!(
                "publication name {name:?} must not be a decimal number, which names a \
                 publication by id",
            ),
        });
    }
    Ok(name)
}

/// The response a retry of an already committed request gets: the outcome
/// that request recorded in `catalog`, the one the retry targets. The caller
/// authorizes the retry as a publish first.
pub(in super::super) async fn replayed_outcome(
    state: &AppState,
    catalog: &str,
    request: Option<&PublicationRequest>,
) -> Result<Option<Response>, RegistryError> {
    let (Some(request), Some(database)) = (request, state.inner.storage.sqlite_metadata()) else {
        return Ok(None);
    };
    let Some(outcome) = database.recorded_outcome(catalog, request).await? else {
        return Ok(None);
    };
    let receipt = PublishReceipt {
        generation: Some(Generation { catalog: outcome.catalog, value: outcome.generation }),
        publication: outcome.publication,
    };
    Ok(Some(publish_committed_response(&receipt)))
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
    let orgs = staged
        .iter()
        .map(|stage| stage.org.as_deref().unwrap_or_default());
    let catalog = match single_catalog(orgs) {
        Ok(catalog) => catalog,
        Err(err) => {
            for stage in staged {
                super::cleanup_tmp_slots(stage.slots).await;
            }
            return Err(err);
        }
    };
    let promoted = promote_archives(state, &mut staged).await?;
    let artifacts = promoted.artifacts.clone();
    let documents = staged
        .into_iter()
        .map(|stage| BatchDocument {
            ecosystem: stage.ecosystem,
            name: stage.name.as_str().to_string(),
            bytes: stage.document,
            expected: stage.base_version,
            principal: stage.publisher,
        })
        .collect();
    let commit = match database.commit_publication(&catalog, documents, artifacts, request).await {
        Ok(commit) => commit,
        Err(err) => {
            promoted.remove_written(state).await;
            return Err(err);
        }
    };
    Ok(PublishReceipt { generation: Some(commit.generation), publication: commit.publication })
}

/// The one organization every package of a publish belongs to. A
/// transaction never spans organizations.
pub(in super::super) fn single_catalog<'org>(
    mut orgs: impl Iterator<Item = &'org str>,
) -> Result<String, RegistryError> {
    let first = orgs.next().unwrap_or_default();
    if orgs.any(|org| org != first) {
        return Err(RegistryError::BadRequest {
            reason: "a publish spans more than one organization; publish each one separately"
                .to_string(),
        });
    }
    Ok(first.to_string())
}

/// The archives a publish promoted before its metadata commit.
#[derive(Default)]
struct Promoted {
    /// The archives of other ecosystems than npm, whose publication members
    /// the metadata cannot derive.
    artifacts: Vec<ArtifactMember>,
    /// The archives this publish wrote, rather than found in place.
    written:
        Vec<(String, pnpr_registry::Ecosystem, pnpr_package_name::CanonicalPackageName, String)>,
}

impl Promoted {
    /// Remove the archives this publish wrote, after its commit was refused,
    /// so no archive outlives the metadata that would have referenced it.
    /// The caller holds every package's lock, so no other publish has
    /// committed one of them since. An archive found in place belongs to an
    /// earlier commit and stays.
    async fn remove_written(&self, state: &AppState) {
        for (org, ecosystem, name, filename) in &self.written {
            let storage = state.inner.storage.for_hosted(org, *ecosystem);
            if let Err(err) = storage.remove_hosted_blob(name, filename).await {
                tracing::warn!(package = %name.as_str(), %filename, %err, "refused publish left its archive");
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
        let finalized =
            state.inner.storage.for_hosted(&org, ecosystem).finalize_blob_slot(slot).await;
        match finalized {
            Ok(BlobFinalize::Written) => {
                promoted.written.push((org, ecosystem, name.clone(), filename.clone()));
            }
            Ok(BlobFinalize::AlreadyIdentical) => {}
            outcome => {
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

/// A publication member for an archive whose version the metadata does not
/// tie it to.
fn artifact_member(
    ecosystem: pnpr_registry::Ecosystem,
    name: &pnpr_package_name::CanonicalPackageName,
    artifact: String,
) -> ArtifactMember {
    ArtifactMember { ecosystem, name: name.as_str().to_string(), version: String::new(), artifact }
}

/// The digest of a request, independent of the order of its object keys. The
/// canonical JSON streams into the hasher, so a body that runs to the
/// request limit is never copied.
pub(super) fn request_digest(body: &Value) -> String {
    let mut hasher = Hasher(IntegrityOpts::new().algorithm(Algorithm::Sha256));
    // Writing into the hasher cannot fail.
    let _ = write_canonical(&mut hasher, body);
    hasher.0.result().to_string()
}

/// Feeds what is written to it into a digest.
struct Hasher(IntegrityOpts);

impl io::Write for Hasher {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.input(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Write `value` as compact JSON with every object's keys sorted.
fn write_canonical(out: &mut Hasher, value: &Value) -> io::Result<()> {
    match value {
        Value::Object(fields) => {
            let mut sorted: Vec<_> = fields.iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(right.0));
            write_sequence(out, (b"{", b"}"), sorted, |out, (key, field)| {
                serde_json::to_writer(&mut *out, key)?;
                out.write_all(b":")?;
                write_canonical(out, field)
            })
        }
        Value::Array(items) => write_sequence(out, (b"[", b"]"), items, write_canonical),
        scalar => Ok(serde_json::to_writer(out, scalar)?),
    }
}

/// Write `items` between `brackets`, separated by commas.
fn write_sequence<Item>(
    out: &mut Hasher,
    brackets: (&[u8], &[u8]),
    items: impl IntoIterator<Item = Item>,
    mut write_item: impl FnMut(&mut Hasher, Item) -> io::Result<()>,
) -> io::Result<()> {
    out.write_all(brackets.0)?;
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            out.write_all(b",")?;
        }
        write_item(out, item)?;
    }
    out.write_all(brackets.1)
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
