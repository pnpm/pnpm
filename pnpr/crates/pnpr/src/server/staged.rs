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
//!
//! Records live in the hosted store, which every replica of a deployment
//! shares, so an approval claims the record it is about to replay: a stage is
//! approved once no matter which replica each request reaches.

mod list_query;
use list_query::{MAX_PER_PAGE, StagedListQuery, parse_staged_list_query};

mod approval;
use approval::serve_staged_approve;

use axum::{
    body::Body,
    extract::{OriginalUri, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Action, AppState, AuthedCaller, Identity, RegistrySource, TargetRegistry, authorize,
    commit_publishes, json_response, not_found, private_no_cache,
    publishing::{ValidatedPublish, cleanup_tmp_slots, report_unrecorded},
    resolve_write_target, stage_publish, validate_publish_doc,
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_search::percent_decode;
use pnpr_storage::{
    DocumentWrite,
    publish::{extract_attachments, now_iso},
};
use std::time::Duration;

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
    /// When the approval holding this record started, if one holds it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    approving_since: Option<String>,
}

/// A staged record as the store holds it. The bytes are what a conditional
/// rewrite compares against, so they are the stored ones rather than a
/// re-serialization of `record`.
struct StoredStagedRecord {
    bytes: Vec<u8>,
    record: StagedRecord,
}

/// A staged record this request has claimed for approval, with the bytes on
/// both sides of the claim so it can be released if the approval fails.
struct ApprovalClaim {
    record: StagedRecord,
    unclaimed_bytes: Vec<u8>,
    claimed_bytes: Vec<u8>,
}

/// How long a claim on a staged record is honored. An approval releases its
/// claim on every outcome, so the lease only matters when the replica holding
/// one died mid-approval: after it the record can be approved again instead of
/// being stranded until someone rejects it. An approval slower than the lease
/// can therefore be joined by a second one, which is bounded rather than
/// unsafe: both replay the same held bytes onto the same immutable blob slot
/// and merge the same version into the document.
const APPROVAL_CLAIM_LEASE: Duration = Duration::from_mins(10);

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

// ---------------------------------------------------------------------
// Route handlers. Each is registered both bare and under `/~{registry}`;
// `TargetRegistry` reports which form the request arrived on.
// ---------------------------------------------------------------------

/// Path capture of the staged routes that address one record. Named rather
/// than a bare `Path<String>` because the prefixed registration captures the
/// `{registry}` segment too, which a single-value `Path` would refuse.
#[derive(Deserialize)]
pub(super) struct StageIdPath {
    id: String,
}

#[derive(Deserialize)]
pub(super) struct StagePackagePath {
    name: String,
}

pub(super) async fn post_staged_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<StagePackagePath>,
    body: axum::body::Bytes,
) -> Response {
    serve_staged_publish(&state, &identity, registry.as_deref(), &path.name, &body).await
}

pub(super) async fn list_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let query = parse_staged_list_query(uri.query().unwrap_or(""));
    private_no_cache(serve_staged_list(&state, &identity, registry.as_deref(), &query).await)
}

pub(super) async fn get_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<StageIdPath>,
) -> Response {
    private_no_cache(serve_staged_view(&state, &identity, registry.as_deref(), &path.id).await)
}

pub(super) async fn reject_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<StageIdPath>,
) -> Response {
    serve_staged_reject(&state, &identity, registry.as_deref(), &path.id).await
}

pub(super) async fn approve_staged(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<StageIdPath>,
) -> Response {
    serve_staged_approve(&state, &identity, registry.as_deref(), &path.id).await
}

pub(super) async fn get_staged_tarball(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<StageIdPath>,
) -> Response {
    private_no_cache(serve_staged_tarball(&state, &identity, registry.as_deref(), &path.id).await)
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
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let incoming: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
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

    // The same routing + `publish`-rule + attachment validation a direct
    // publish runs; conflicts with already-published versions are checked
    // when the stage is approved, against the registry state at that time.
    let (validated, _target) =
        match validate_publish_doc(state, identity, registry, name, incoming).await {
            Ok(validated) => validated,
            Err(err) => return err.into_response(),
        };

    let stage_id = generate_stage_id();
    let record = staged_record(&validated, identity, registry, &stage_id);

    if let Err(err) = store_staged(state, &stage_id, body, &record).await {
        return err.into_response();
    }
    json_response(StatusCode::CREATED, &json!({ "ok": true, "stageId": stage_id }))
}

fn staged_record(
    validated: &ValidatedPublish,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> StagedRecord {
    let (version, dist) = validated.prepared.first().map_or((None, Value::Null), |attachment| {
        (Some(attachment.version.clone()), attachment.dist.clone())
    });
    let (actor, actor_type) = actor_of(identity);
    StagedRecord {
        id: stage_id.to_string(),
        package_name: validated.name.as_str().to_string(),
        tag: staged_tag(&validated.incoming, version.as_deref()),
        version,
        created_at: now_iso(),
        actor,
        actor_type,
        shasum: dist.get("shasum").and_then(Value::as_str).map(str::to_string),
        registry: registry.map(str::to_string),
        approving_since: None,
    }
}

/// The dist-tag naming the staged version, else the first tag declared.
fn staged_tag(incoming: &Value, version: Option<&str>) -> Option<String> {
    let tags = incoming.get("dist-tags").and_then(Value::as_object)?;
    match version {
        Some(version) => tags
            .iter()
            .find(|(_, tagged)| tagged.as_str() == Some(version))
            .or_else(|| tags.iter().next())
            .map(|(tag, _)| tag.clone()),
        None => tags.keys().next().cloned(),
    }
}

/// Body first, metadata last: a record whose metadata exists always has
/// its body. On a metadata failure the body is cleaned up best-effort.
async fn store_staged(
    state: &AppState,
    stage_id: &str,
    body: &axum::body::Bytes,
    record: &StagedRecord,
) -> Result<(), RegistryError> {
    state.inner.storage.create_staged_body(stage_id, body).await?;
    let meta_bytes = serde_json::to_vec(record).expect("a staged record serializes");
    if let Err(err) = state.inner.storage.create_staged_meta(stage_id, &meta_bytes).await {
        let _ = state.inner.storage.remove_staged(stage_id).await;
        return Err(err);
    }
    Ok(())
}

/// `GET /-/stage?page=&perPage=&package=` — the staged records visible to
/// the caller through this registry address, sorted oldest-first by staging
/// time (then id) so pagination stays stable as new records arrive.
async fn serve_staged_list(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    query: &StagedListQuery,
) -> Response {
    let per_page = query.per_page.clamp(1, MAX_PER_PAGE);
    let ids = match state.inner.storage.list_staged_ids().await {
        Ok(ids) => ids,
        Err(err) => return err.into_response(),
    };
    let mut records: Vec<StagedRecord> = Vec::new();
    for stage_id in ids {
        let Ok(Some(stored)) = read_staged_record(state, &stage_id).await else {
            continue;
        };
        let record = stored.record;
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
    let items = metadata_page(&records, query.page, per_page);
    json_response(
        StatusCode::OK,
        &json!({ "items": items, "page": query.page, "perPage": per_page, "total": total }),
    )
}

/// The `page`th page of `records`, `per_page` records long, as metadata
/// documents.
fn metadata_page(records: &[StagedRecord], page: usize, per_page: usize) -> Vec<Value> {
    let selected = records.iter().skip(page.saturating_mul(per_page)).take(per_page);
    selected.map(StagedRecord::metadata).collect()
}

/// `GET /-/stage/:id` — one staged record's metadata.
async fn serve_staged_view(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    let stored = match load_authorized_record(state, identity, registry, stage_id).await {
        Ok(stored) => stored,
        Err(err) => return err.into_response(),
    };
    json_response(StatusCode::OK, &stored.record.metadata())
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
        return response.into_response();
    }
    match state.inner.storage.remove_staged(stage_id).await {
        Ok(_) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .expect("static-shape response always builds"),
        Err(err) => err.into_response(),
    }
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
        return response.into_response();
    }
    let body = match state.inner.storage.read_staged_body(stage_id).await {
        Ok(Some(body)) => body,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let mut incoming: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let attachments = match extract_attachments(&mut incoming) {
        Ok(attachments) => attachments,
        Err(err) => return err.into_response(),
    };
    let Some(attachment) = attachments.into_iter().next() else {
        return not_found();
    };
    let bytes = match BASE64.decode(attachment.data.as_bytes()) {
        Ok(bytes) => bytes,
        Err(err) => {
            return RegistryError::InvalidAttachment {
                filename: attachment.filename,
                reason: format!("invalid base64 data: {err}"),
            }
            .into_response();
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
) -> Result<StoredStagedRecord, RegistryError> {
    let stored = match read_staged_record(state, stage_id).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return Err(RegistryError::NotFound),
        Err(err) => return Err(err),
    };
    if stored.record.registry.as_deref() != registry {
        return Err(RegistryError::NotFound);
    }
    authorize_staged(state, identity, &stored.record).await?;
    Ok(stored)
}

/// The `publish` authorization a staged record's package demands, resolved
/// through the registry prefix the record was staged with.
async fn authorize_staged(
    state: &AppState,
    identity: &Identity,
    record: &StagedRecord,
) -> Result<(), RegistryError> {
    let name =
        CanonicalPackageName::parse(&record.package_name, pnpr_package_name::Ecosystem::Npm)?;
    let target = resolve_write_target(state, identity, record.registry.as_deref(), &name)?;
    authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source),
        name.as_str(),
        Action::Publish,
    )
}

async fn read_staged_record(
    state: &AppState,
    stage_id: &str,
) -> Result<Option<StoredStagedRecord>, RegistryError> {
    let Some(bytes) = state.inner.storage.read_staged_meta(stage_id).await? else {
        return Ok(None);
    };
    let record = serde_json::from_slice(&bytes).map_err(RegistryError::Json)?;
    Ok(Some(StoredStagedRecord { bytes, record }))
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
