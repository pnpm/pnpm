//! The `-/stage` endpoints: staged (two-phase) publishing.
//!
//! `POST /-/stage/package/:pkg` accepts a regular publish document but holds
//! it back instead of making it visible; the staged record is then listed
//! (`GET /-/stage`), inspected (`GET /-/stage/:id`, `GET
//! /-/stage/:id/tarball`), and finally approved (`POST /-/stage/:id/approve`
//! — which replays the held document through the regular publish flow) or
//! rejected (`DELETE /-/stage/:id` — which deletes it). This is the server
//! half of `pnpm stage`.
//!
//! Every operation is gated by the same `publish` rule as a direct publish
//! of the package, resolved through the registry prefix the stage was
//! addressed with. Stage ids are random UUIDs, so the id itself is an
//! unguessable capability; denials answer loudly (401/403) like the publish
//! endpoint rather than masking.

use axum::{
    body::Body,
    extract::{OriginalUri, Path, State},
    http::{StatusCode, header},
    response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Action, AppState, AuthedCaller, Identity, RegistrySource, authorize, commit_publishes,
    error_response, is_tilde_prefix, json_response, not_found, private_no_cache,
    resolve_write_target, stage_publish, validate_publish_doc,
};
use crate::{
    error::RegistryError,
    package_name::PackageName,
    publish::{extract_attachments, now_iso},
    search::percent_decode,
};

/// One staged publish's metadata, stored next to the held publish body and
/// served by the list/view endpoints (without the `registry` field, which is
/// routing state rather than metadata).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedRecord {
    id: String,
    package_name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    tag: Option<String>,
    created_at: String,
    actor: String,
    actor_type: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    shasum: Option<String>,
    /// The `/~<name>/` registry prefix the stage was addressed through;
    /// `None` for the path-less base. A staged record is only visible
    /// through the same address it was created with.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    registry: Option<String>,
}

impl StagedRecord {
    /// The list/view representation: the record without its routing state.
    fn metadata(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("a staged record serializes");
        if let Some(object) = value.as_object_mut() {
            object.remove("registry");
        }
        value
    }
}

const DEFAULT_PER_PAGE: usize = 100;
const MAX_PER_PAGE: usize = 100;

#[derive(Debug)]
struct StagedListQuery {
    page: usize,
    per_page: usize,
    package: Option<String>,
}

/// Parse the list endpoint's `page` / `perPage` / `package` query
/// parameters, ignoring anything unrecognized or unparsable.
fn parse_staged_list_query(query: &str) -> StagedListQuery {
    let mut parsed = StagedListQuery { page: 0, per_page: DEFAULT_PER_PAGE, package: None };
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let decoded = percent_decode(value);
        match key {
            "page" => {
                if let Ok(page) = decoded.parse() {
                    parsed.page = page;
                }
            }
            "perPage" => {
                if let Ok(per_page) = decoded.parse() {
                    parsed.per_page = per_page;
                }
            }
            "package" if !decoded.is_empty() => parsed.package = Some(decoded),
            _ => {}
        }
    }
    parsed
}

// ---------------------------------------------------------------------
// Route handlers — the path-less form and its `/~<name>/`-prefixed twin.
// ---------------------------------------------------------------------

pub(super) async fn post_staged_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    serve_staged_publish(&state, &identity, None, &name, &body).await
}

pub(super) async fn post_staged_publish_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, name)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    match tilde_registry(&prefix) {
        Some(registry) => {
            serve_staged_publish(&state, &identity, Some(registry), &name, &body).await
        }
        None => not_found(),
    }
}

pub(super) async fn list_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let query = parse_staged_list_query(uri.query().unwrap_or(""));
    private_no_cache(serve_staged_list(&state, &identity, None, &query).await)
}

pub(super) async fn list_staged_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
    Path(prefix): Path<String>,
) -> Response {
    match tilde_registry(&prefix) {
        Some(registry) => {
            let query = parse_staged_list_query(uri.query().unwrap_or(""));
            private_no_cache(serve_staged_list(&state, &identity, Some(registry), &query).await)
        }
        None => not_found(),
    }
}

pub(super) async fn get_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(stage_id): Path<String>,
) -> Response {
    private_no_cache(serve_staged_view(&state, &identity, None, &stage_id).await)
}

pub(super) async fn get_staged_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, stage_id)): Path<(String, String)>,
) -> Response {
    match tilde_registry(&prefix) {
        Some(registry) => {
            private_no_cache(serve_staged_view(&state, &identity, Some(registry), &stage_id).await)
        }
        None => not_found(),
    }
}

pub(super) async fn reject_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(stage_id): Path<String>,
) -> Response {
    serve_staged_reject(&state, &identity, None, &stage_id).await
}

pub(super) async fn reject_staged_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, stage_id)): Path<(String, String)>,
) -> Response {
    match tilde_registry(&prefix) {
        Some(registry) => serve_staged_reject(&state, &identity, Some(registry), &stage_id).await,
        None => not_found(),
    }
}

pub(super) async fn approve_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(stage_id): Path<String>,
) -> Response {
    serve_staged_approve(&state, &identity, None, &stage_id).await
}

pub(super) async fn approve_staged_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, stage_id)): Path<(String, String)>,
) -> Response {
    match tilde_registry(&prefix) {
        Some(registry) => serve_staged_approve(&state, &identity, Some(registry), &stage_id).await,
        None => not_found(),
    }
}

pub(super) async fn get_staged_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(stage_id): Path<String>,
) -> Response {
    private_no_cache(serve_staged_tarball(&state, &identity, None, &stage_id).await)
}

pub(super) async fn get_staged_tarball_prefixed(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((prefix, stage_id)): Path<(String, String)>,
) -> Response {
    match tilde_registry(&prefix) {
        Some(registry) => private_no_cache(
            serve_staged_tarball(&state, &identity, Some(registry), &stage_id).await,
        ),
        None => not_found(),
    }
}

/// The registry a `/~<name>/`-prefixed staged route addresses, or `None`
/// when the first segment isn't a tilde prefix (the route pattern also
/// matches plain segments; those name no staged surface).
fn tilde_registry(prefix: &str) -> Option<&str> {
    is_tilde_prefix(prefix).then(|| &prefix[1..])
}

// ---------------------------------------------------------------------
// The handlers proper.
// ---------------------------------------------------------------------

/// `POST /-/stage/package/:pkg` — validate and authorize the publish
/// document exactly like a direct publish, then hold it back under a fresh
/// stage id instead of committing it.
async fn serve_staged_publish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    body: &axum::body::Bytes,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(name) => name,
        Err(err) => return error_response(&err),
    };
    let incoming: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let body_name = incoming.get("name").and_then(Value::as_str);
    if body_name.is_some_and(|body_name| body_name != name.as_str()) {
        return error_response(&RegistryError::BadRequest {
            reason: format!(
                "package in URL ({:?}) does not match body ({:?})",
                name.as_str(),
                body_name.unwrap_or(""),
            ),
        });
    }

    // The same routing + `publish`-rule + attachment validation a direct
    // publish runs; conflicts with already-published versions are checked
    // when the stage is approved, against the registry state at that time.
    let (validated, _target) =
        match validate_publish_doc(state, identity, registry, name, incoming).await {
            Ok(validated) => validated,
            Err(response) => return *response,
        };

    let (version, dist) = validated.prepared.first().map_or((None, Value::Null), |attachment| {
        (Some(attachment.version.clone()), attachment.dist.clone())
    });
    let tag = validated.incoming.get("dist-tags").and_then(Value::as_object).and_then(|tags| {
        match &version {
            Some(version) => tags
                .iter()
                .find(|(_, tagged)| tagged.as_str() == Some(version))
                .or_else(|| tags.iter().next())
                .map(|(tag, _)| tag.clone()),
            None => tags.keys().next().cloned(),
        }
    });
    let (actor, actor_type) = actor_of(identity);
    let stage_id = generate_stage_id();
    let record = StagedRecord {
        id: stage_id.clone(),
        package_name: validated.name.as_str().to_string(),
        version,
        tag,
        created_at: now_iso(),
        actor,
        actor_type,
        shasum: dist.get("shasum").and_then(Value::as_str).map(str::to_string),
        registry: registry.map(str::to_string),
    };

    // Body first, metadata last: a record whose metadata exists always has
    // its body. On a metadata failure the body is cleaned up best-effort.
    if let Err(err) = state.inner.storage.write_staged_body(&stage_id, body).await {
        return error_response(&err);
    }
    let meta_bytes = serde_json::to_vec(&record).expect("a staged record serializes");
    if let Err(err) = state.inner.storage.write_staged_meta(&stage_id, &meta_bytes).await {
        let _ = state.inner.storage.remove_staged(&stage_id).await;
        return error_response(&err);
    }
    json_response(StatusCode::CREATED, &json!({ "ok": true, "stageId": stage_id }))
}

/// `GET /-/stage?page=&perPage=&package=` — the staged records visible to
/// the caller through this registry address, newest-first-insensitive
/// (sorted by staging time, then id, for stable pagination).
async fn serve_staged_list(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    query: &StagedListQuery,
) -> Response {
    let per_page = query.per_page.clamp(1, MAX_PER_PAGE);
    let ids = match state.inner.storage.list_staged_ids().await {
        Ok(ids) => ids,
        Err(err) => return error_response(&err),
    };
    let mut records: Vec<StagedRecord> = Vec::new();
    for stage_id in ids {
        let Ok(Some(record)) = read_staged_record(state, &stage_id).await else {
            continue;
        };
        if record.registry.as_deref() != registry {
            continue;
        }
        if let Some(package) = &query.package
            && &record.package_name != package
        {
            continue;
        }
        // The listing shows only what the caller could publish (and thus
        // approve); records outside their rights are simply not theirs to see.
        if authorize_staged(state, identity, &record).await.is_err() {
            continue;
        }
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.created_at.cmp(&right.created_at).then_with(|| left.id.cmp(&right.id))
    });

    let total = records.len();
    let items: Vec<Value> = records
        .iter()
        .skip(query.page.saturating_mul(per_page))
        .take(per_page)
        .map(StagedRecord::metadata)
        .collect();
    json_response(
        StatusCode::OK,
        &json!({ "items": items, "page": query.page, "perPage": per_page, "total": total }),
    )
}

/// `GET /-/stage/:id` — one staged record's metadata.
async fn serve_staged_view(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    let record = match load_authorized_record(state, identity, registry, stage_id).await {
        Ok(record) => record,
        Err(response) => return *response,
    };
    json_response(StatusCode::OK, &record.metadata())
}

/// `DELETE /-/stage/:id` — reject a staged publish, deleting its record and
/// held tarball.
async fn serve_staged_reject(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    if let Err(response) = load_authorized_record(state, identity, registry, stage_id).await {
        return *response;
    }
    match state.inner.storage.remove_staged(stage_id).await {
        Ok(_) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .expect("static-shape response always builds"),
        Err(err) => error_response(&err),
    }
}

/// `POST /-/stage/:id/approve` — publish the held document through the
/// regular validate → stage → commit flow, then drop the staged record.
async fn serve_staged_approve(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    let record = match load_authorized_record(state, identity, registry, stage_id).await {
        Ok(record) => record,
        Err(response) => return *response,
    };
    let body = match state.inner.storage.read_staged_body(stage_id).await {
        Ok(Some(body)) => body,
        Ok(None) => {
            return error_response(&RegistryError::Io(std::io::Error::other(format!(
                "staged publish {stage_id} has no stored body",
            ))));
        }
        Err(err) => return error_response(&err),
    };
    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let name = match PackageName::parse(&record.package_name) {
        Ok(name) => name,
        Err(err) => return error_response(&err),
    };
    // Re-validate against the registry state of *now*: rules may have
    // changed since staging, and the version may have been published in
    // the meantime (which surfaces as the usual 409).
    let (validated, target) =
        match validate_publish_doc(state, identity, record.registry.as_deref(), name, incoming)
            .await
        {
            Ok(validated) => validated,
            Err(response) => return *response,
        };

    let _packument_guard = state.inner.package_locks.lock(validated.name.as_str()).await;
    let staged = match stage_publish(state, validated, &now_iso(), Some(&target.org)).await {
        Ok(staged) => staged,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = commit_publishes(state, vec![staged]).await {
        return error_response(&err);
    }
    if let Err(err) = state.inner.storage.remove_staged(stage_id).await {
        // The publish is already committed and visible; a failed record
        // cleanup must not report the approval as failed.
        tracing::warn!(error = %err, stage_id, "approved staged publish but its record cleanup failed");
    }
    json_response(StatusCode::CREATED, &json!({ "ok": true }))
}

/// `GET /-/stage/:id/tarball` — the held tarball's bytes, decoded from the
/// stored publish document's attachment.
async fn serve_staged_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    if let Err(response) = load_authorized_record(state, identity, registry, stage_id).await {
        return *response;
    }
    let body = match state.inner.storage.read_staged_body(stage_id).await {
        Ok(Some(body)) => body,
        Ok(None) => return not_found(),
        Err(err) => return error_response(&err),
    };
    let mut incoming: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => return error_response(&RegistryError::Json(err)),
    };
    let attachments = match extract_attachments(&mut incoming) {
        Ok(attachments) => attachments,
        Err(err) => return error_response(&err),
    };
    let Some(attachment) = attachments.into_iter().next() else {
        return not_found();
    };
    let bytes = match BASE64.decode(attachment.data.as_bytes()) {
        Ok(bytes) => bytes,
        Err(err) => {
            return error_response(&RegistryError::InvalidAttachment {
                filename: attachment.filename,
                reason: format!("invalid base64 data: {err}"),
            });
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

// ---------------------------------------------------------------------
// Shared plumbing.
// ---------------------------------------------------------------------

/// Load a staged record and check the caller may act on it: the record must
/// exist, be addressed through the same registry prefix it was staged with,
/// and the caller must hold the `publish` right on its package.
async fn load_authorized_record(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Result<StagedRecord, Box<Response>> {
    let record = match read_staged_record(state, stage_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return Err(Box::new(not_found())),
        Err(err) => return Err(Box::new(error_response(&err))),
    };
    if record.registry.as_deref() != registry {
        return Err(Box::new(not_found()));
    }
    authorize_staged(state, identity, &record).await?;
    Ok(record)
}

/// The `publish` authorization a staged record's package demands, resolved
/// through the registry prefix the record was staged with.
async fn authorize_staged(
    state: &AppState,
    identity: &Identity,
    record: &StagedRecord,
) -> Result<(), Box<Response>> {
    let name =
        PackageName::parse(&record.package_name).map_err(|err| Box::new(error_response(&err)))?;
    let target = resolve_write_target(state, identity, record.registry.as_deref(), &name)?;
    authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source),
        name.as_str(),
        Action::Publish,
    )
    .map_err(|err| Box::new(error_response(&err)))
}

async fn read_staged_record(
    state: &AppState,
    stage_id: &str,
) -> Result<Option<StagedRecord>, RegistryError> {
    let Some(bytes) = state.inner.storage.read_staged_meta(stage_id).await? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes).map(Some).map_err(RegistryError::Json)
}

fn actor_of(identity: &Identity) -> (String, String) {
    match identity {
        Identity::User { username, .. } => (username.clone(), "user".to_string()),
        // Reachable only when the registry's publish rule allows anonymous
        // writes; the record still needs an actor to display.
        Identity::Anonymous => ("anonymous".to_string(), "user".to_string()),
    }
}

/// A fresh random (version 4) UUID from the OS CSPRNG — the stage id
/// clients quote back on view/approve/reject/download.
fn generate_stage_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().fold(String::with_capacity(32), |mut hex, byte| {
        use std::fmt::Write;
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
        hex
    });
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

#[cfg(test)]
mod tests;
