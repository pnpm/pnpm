use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{
    DOCUMENT_WRITE_RETRIES,
    metadata::{Generation, TagChange, TagUpdate, validate_dist_tag},
    wait_after_document_write_conflict,
};

use super::read_authorized_packument;
use crate::server::{
    Action, AppState, GENERATION_HEADER, RegistrySource, authorize_without_owners,
    filter_osv_vulnerable_dist_tags, hosted_storage, load_packument_for_read, not_found,
    resolve_write_target,
};

/// `GET /-/package/:pkg/dist-tags` (path-less) or
/// `GET /~<name>/-/package/:pkg/dist-tags` — return the packument's
/// `dist-tags` object.
pub(crate) async fn get_dist_tags(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };
    let bytes = match load_packument_for_read(state, identity, registry, &name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let packument: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let mut tags = packument
        .get("dist-tags")
        .cloned()
        .unwrap_or_else(|| json!({}));
    filter_osv_vulnerable_dist_tags(&mut tags, &packument, &name, state.inner.osv_index.as_ref());
    let bytes = serde_json::to_vec(&tags).expect("dist-tags object serializes");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `PUT /-/package/:pkg/dist-tags/:tag` (path-less) or
/// `PUT /~<name>/-/package/:pkg/dist-tags/:tag` — set a dist-tag. Body is
/// a JSON-encoded version string (e.g. `"1.0.0"`), read only once the caller
/// may change the tag.
pub(crate) async fn set_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    body: &[u8],
) -> Response {
    let request = TagRequest { registry, raw_name, tag, body: Some(body) };
    match update_dist_tag(state, identity, request).await {
        Ok(outcome) => {
            log_tag_change(identity, &request, &outcome);
            outcome.response
        }
        Err(response) => response,
    }
}

pub(crate) async fn remove_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
) -> Response {
    let request = TagRequest { registry, raw_name, tag, body: None };
    match update_dist_tag(state, identity, request).await {
        Ok(outcome) => {
            if outcome.moved {
                log_tag_change(identity, &request, &outcome);
            }
            outcome.response
        }
        Err(response) => response,
    }
}

/// A dist-tag change as the request asked for it.
#[derive(Clone, Copy)]
struct TagRequest<'request> {
    registry: Option<&'request str>,
    raw_name: &'request str,
    tag: &'request str,
    /// The body naming the version to point the tag at, or `None` to remove
    /// the tag.
    body: Option<&'request [u8]>,
}

/// A committed dist-tag change.
struct TagOutcome {
    response: Response,
    /// The hosted registry that holds the package.
    source: String,
    /// The version the tag now points at, `None` once removed.
    version: Option<String>,
    moved: bool,
}

/// Authorize a dist-tag change against the stored packument and commit it at
/// that generation. Answers 201 with the committed generation.
async fn update_dist_tag(
    state: &AppState,
    identity: &Identity,
    request: TagRequest<'_>,
) -> Result<TagOutcome, Response> {
    validate_dist_tag(request.tag).map_err(IntoResponse::into_response)?;
    let name = CanonicalPackageName::parse(request.raw_name, pnpr_package_name::Ecosystem::Npm)
        .map_err(IntoResponse::into_response)?;
    let target = resolve_write_target(state, identity, request.registry, &name)
        .map_err(IntoResponse::into_response)?;
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    for attempt in 0..DOCUMENT_WRITE_RETRIES {
        match try_update_dist_tag(state, identity, (&target, &name), request).await? {
            Some(outcome) => return Ok(outcome),
            None if attempt + 1 < DOCUMENT_WRITE_RETRIES => {
                wait_after_document_write_conflict(attempt).await;
            }
            None => {}
        }
    }
    Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() }.into_response())
}

async fn try_update_dist_tag(
    state: &AppState,
    identity: &Identity,
    (target, name): (&crate::server::WriteTarget, &CanonicalPackageName),
    request: TagRequest<'_>,
) -> Result<Option<TagOutcome>, Response> {
    let source = RegistrySource::Hosted(target.source.clone());
    let expected =
        if authorize_without_owners(state, identity, &source, name.as_str(), Action::Publish)
            .map_err(IntoResponse::into_response)?
        {
            None
        } else {
            let Some((_, version)) =
                read_authorized_packument(state, identity, (target, name), Action::Publish)
                    .await
                    .map_err(IntoResponse::into_response)?
            else {
                return Err(not_found());
            };
            Some(version)
        };
    let version = parse_tag_target(request.body).map_err(IntoResponse::into_response)?;
    let change = TagChange {
        tag: request.tag,
        target: version.as_deref(),
        principal: identity.user_id(),
        expected: expected.as_ref(),
    };
    let storage = hosted_storage(state, Some(&target.namespace));
    match storage.change_dist_tag(name, change).await.map_err(IntoResponse::into_response)? {
        TagUpdate::Committed { generation, moved } => Ok(Some(TagOutcome {
            response: ok_created_response(generation.as_ref()),
            source: target.source.clone(),
            version,
            moved,
        })),
        TagUpdate::NotFound => Err(not_found()),
        TagUpdate::Conflict => Ok(None),
    }
}

fn parse_tag_target(body: Option<&[u8]>) -> Result<Option<String>, RegistryError> {
    body.map(serde_json::from_slice::<String>)
        .transpose()
        .map_err(|_| RegistryError::BadRequest {
            reason: "dist-tag target must be a JSON string".to_string(),
        })
}

fn log_tag_change(identity: &Identity, request: &TagRequest<'_>, outcome: &TagOutcome) {
    let actor = identity.username().unwrap_or("-");
    let source = &outcome.source;
    if let Some(version) = &outcome.version {
        tracing::info!(
            target: "pnpr::audit",
            action = "package.dist_tag_set",
            actor = %actor,
            package = %request.raw_name,
            tag = %request.tag,
            version = %version,
            registry = %source,
            "package dist-tag set",
        );
    } else {
        tracing::info!(
            target: "pnpr::audit",
            action = "package.dist_tag_remove",
            actor = %actor,
            package = %request.raw_name,
            tag = %request.tag,
            registry = %source,
            "package dist-tag removed",
        );
    }
}

fn ok_created_response(generation: Option<&Generation>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(generation) = generation {
        builder = builder.header(GENERATION_HEADER, generation.to_string());
    }
    builder
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static-shape response always builds")
}
