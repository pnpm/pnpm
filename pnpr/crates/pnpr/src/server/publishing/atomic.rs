//! Committing a publish as one metadata transaction: every archive is
//! promoted first, then every document of the request commits under one
//! generation of its organization's catalog, or none does. Readers see the
//! whole request or nothing of it.
//!
//! The request's outcome commits with it. A retry of the same request by the
//! same principal gets that outcome back instead of a conflict, whether or not
//! the client sent an idempotency key: without one, the digest of the whole
//! request is the key. Unpublishing an npm version or deleting an OCI manifest
//! by digest invalidates its outcome.
//! A retry is authorized against current permissions
//! like any publish before its outcome is looked up.

use super::{RegistryError, StagedPublish, Value, promotion::promote_fenced};
use crate::server::{AppState, Identity};
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{
    metadata::{BatchDocument, Generation, PublicationCommit, PublicationRequest, PublishNotes},
    transparency_log::LogId,
};
use serde_json::json;
use ssri::{Algorithm, IntegrityOpts};
use std::io::{self, Write as _};

/// What a publish committed, as its response reports it.
#[derive(Debug, Default)]
pub(in super::super) struct PublishReceipt {
    /// The generation holding the publish.
    pub(in super::super) generation: Option<Generation>,
    /// The publication the commit recorded.
    pub(in super::super) publication: Option<u64>,
    /// What the publisher should hear about a version accepted with a
    /// difference, sent back as `npm-notice` response headers.
    pub(in super::super) notices: Vec<String>,
}

impl From<PublicationCommit> for PublishReceipt {
    fn from(commit: PublicationCommit) -> Self {
        Self {
            generation: Some(commit.generation),
            publication: commit.publication,
            notices: Vec::new(),
        }
    }
}

/// The idempotency claim of a publish request: who sent it, the request it
/// is a retry of when it is one, and the publication's `name`. `None` for a
/// caller with no stable principal.
pub(in super::super) fn publication_request(
    identity: &Identity,
    body: &Value,
    key: Option<&str>,
    name: Option<&str>,
    target: Option<&CanonicalPackageName>,
) -> Option<PublicationRequest> {
    let principal = identity.user_id()?.to_string();
    let request_digest = request_digest(body, target.map(CanonicalPackageName::as_str));
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
    let Some(request) = request else { return Ok(None) };
    let database = state.inner.storage.sql_metadata();
    let Some(outcome) = database.recorded_outcome(catalog, request).await? else {
        return Ok(None);
    };
    let receipt = PublishReceipt {
        generation: Some(Generation { catalog: outcome.catalog, value: outcome.generation }),
        publication: outcome.publication,
        notices: Vec::new(),
    };
    Ok(Some(publish_committed_response(&receipt)))
}

/// Commit `staged` in one metadata transaction. The receipt carries the
/// notices staging left for the publisher.
pub(in super::super) async fn commit_publication(
    state: &AppState,
    mut staged: Vec<StagedPublish>,
    notes: PublishNotes,
    request: Option<PublicationRequest>,
) -> Result<PublishReceipt, RegistryError> {
    let notices = staged
        .iter_mut()
        .flat_map(|stage| std::mem::take(&mut stage.verification.notices))
        .collect();
    let receipt = commit_staged(state, staged, notes, request).await?;
    Ok(PublishReceipt { notices, ..receipt })
}

async fn commit_staged(
    state: &AppState,
    mut staged: Vec<StagedPublish>,
    notes: PublishNotes,
    request: Option<PublicationRequest>,
) -> Result<PublishReceipt, RegistryError> {
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
    let promoted = promote_fenced(state, &mut staged).await?;
    let rebases = super::rebase::rebase_bases(&mut staged);
    let (documents, logged) = batch_documents(staged);
    let publication = super::rebase::Publication {
        catalog: &catalog,
        artifacts: promoted.artifacts.clone(),
        notes,
        request,
        fence: promoted.fence.clone(),
    };
    let commit = match super::rebase::commit_rebasing(state, publication, documents, rebases).await
    {
        Ok(commit) => commit,
        Err(err) => {
            promoted.discard(state).await;
            return Err(err);
        }
    };
    if !commit.replayed {
        for ecosystem in logged {
            let log = LogId { catalog: catalog.clone(), ecosystem };
            super::super::release_record_logs::integrate_in_background(state, log);
        }
    }
    Ok(commit.into())
}

/// The documents a commit writes, and the ecosystems whose logs the records
/// they bring are sequenced in.
fn batch_documents(
    staged: Vec<StagedPublish>,
) -> (Vec<BatchDocument>, std::collections::BTreeSet<pnpr_package_name::Ecosystem>) {
    let logged = staged
        .iter()
        .filter(|stage| !stage.verification.records.is_empty())
        .map(|stage| stage.ecosystem)
        .collect();
    let documents = staged
        .into_iter()
        .map(|stage| BatchDocument {
            ecosystem: stage.ecosystem,
            name: stage.name.as_str().to_string(),
            bytes: stage.document,
            expected: stage.base.version,
            principal: stage.publisher,
            records: stage.verification.records,
        })
        .collect();
    (documents, logged)
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

/// The digest of a request, independent of the order of its object keys:
/// its body, and the package its URL names when the route names one, since
/// the same body sent to another package is another publish. The canonical
/// JSON streams into the hasher, so a body that runs to the request limit is
/// never copied.
pub(super) fn request_digest(body: &Value, target: Option<&str>) -> String {
    let mut hasher = Hasher(IntegrityOpts::new().algorithm(Algorithm::Sha256));
    // Writing into the hasher cannot fail.
    let _ = match target {
        None => write_canonical(&mut hasher, body),
        Some(target) => write_targeted(&mut hasher, body, target),
    };
    hasher.0.result().to_string()
}

/// `{"body":<body>,"target":<target>}`, canonical.
fn write_targeted(out: &mut Hasher, body: &Value, target: &str) -> io::Result<()> {
    out.write_all(br#"{"body":"#)?;
    write_canonical(out, body)?;
    out.write_all(br#","target":"#)?;
    serde_json::to_writer(&mut *out, target)?;
    out.write_all(b"}")
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
/// it and the publication it recorded, with its notices.
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
    let mut response = builder
        .body(Body::from(body.to_string()))
        .expect("static-shape response always builds");
    add_npm_notices(&mut response, &receipt.notices);
    response
}

/// The most bytes of `npm-notice` values one response carries, so a publish
/// of many versions, or of long names, cannot grow its headers past what a
/// proxy or client accepts.
const NPM_NOTICE_BYTES: usize = 2048;

/// The bytes held back from [`NPM_NOTICE_BYTES`] for the summary of the
/// notices that do not fit: its text with the longest count.
const NPM_NOTICE_SUMMARY_BYTES: usize = 64;

/// Send `notices` in `npm-notice` headers, which npm prints to the
/// publisher: those that fit in [`NPM_NOTICE_BYTES`] beside a count of the
/// rest.
pub(in super::super) fn add_npm_notices(response: &mut Response, notices: &[String]) {
    let mut budget = NPM_NOTICE_BYTES - NPM_NOTICE_SUMMARY_BYTES;
    let mut sent = 0;
    for notice in notices {
        let Some(left) = budget.checked_sub(notice.len()) else { break };
        let Ok(value) = header::HeaderValue::from_str(notice) else { break };
        response.headers_mut().append("npm-notice", value);
        budget = left;
        sent += 1;
    }
    let rest = notices.len() - sent;
    if rest > 0 {
        let summary = format!("{rest} more versions were accepted with a notice");
        response
            .headers_mut()
            .append(
                "npm-notice",
                header::HeaderValue::from_str(&summary)
                    .expect("an ASCII summary is a header value"),
            );
    }
}
