use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::metadata::{Generation, TagChange, TagUpdate};

use crate::server::{
    Action, AppState, GENERATION_HEADER, RegistrySource, authorize_package,
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

fn validate_dist_tag(tag: &str) -> Result<(), RegistryError> {
    if tag.is_empty()
        || tag
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(RegistryError::BadRequest { reason: "invalid dist-tag name".to_string() });
    }
    Ok(())
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

/// Authorize a dist-tag change and commit it. Answers 201 with the committed
/// generation, when the metadata backend tracks one.
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
    authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
        Some(&target.org),
    )
    .await
    .map_err(IntoResponse::into_response)?;
    let version = request.body
        .map(serde_json::from_slice::<String>)
        .transpose()
        .map_err(|err| RegistryError::Json(err).into_response())?;
    let storage = hosted_storage(state, Some(&target.org));
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let change =
        TagChange { tag: request.tag, target: version.as_deref(), principal: identity.username() };
    match storage.change_dist_tag(&name, change).await {
        Ok(TagUpdate::Committed { generation, moved }) => Ok(TagOutcome {
            response: ok_created_response(generation.as_ref()),
            source: target.source,
            version,
            moved,
        }),
        Ok(TagUpdate::NotFound) => Err(not_found()),
        Err(err) => Err(err.into_response()),
    }
}

fn log_tag_change(identity: &Identity, request: &TagRequest<'_>, outcome: &TagOutcome) {
    let actor = identity.username().unwrap_or("-");
    let source = &outcome.source;
    match &outcome.version {
        Some(version) => tracing::info!(
            target: "pnpr::audit",
            action = "package.dist_tag_set",
            actor = %actor,
            package = %request.raw_name,
            tag = %request.tag,
            version = %version,
            registry = %source,
            "package dist-tag set",
        ),
        None => tracing::info!(
            target: "pnpr::audit",
            action = "package.dist_tag_remove",
            actor = %actor,
            package = %request.raw_name,
            tag = %request.tag,
            registry = %source,
            "package dist-tag removed",
        ),
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
