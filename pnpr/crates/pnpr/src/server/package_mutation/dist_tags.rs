use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate, publish::now_iso};

use crate::server::{
    Action, AppState, RegistrySource, authorize_package, filter_osv_vulnerable_dist_tags,
    hosted_storage, load_packument_for_read, not_found, resolve_write_target,
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
/// a JSON-encoded version string (e.g. `"1.0.0"`).
pub(crate) async fn set_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    body: &[u8],
) -> Response {
    if let Err(err) = validate_dist_tag(tag) {
        return err.into_response();
    }
    let mut captured_version = None;
    let (res, source) = update_dist_tag(state, identity, registry, raw_name, tag, |tags| {
        let version: String = serde_json::from_slice(body).map_err(RegistryError::Json)?;
        captured_version = Some(version.clone());
        tags.insert(tag.to_string(), Value::String(version));
        Ok(())
    })
    .await;
    if let Some(source) = &source {
        let actor = identity.username().unwrap_or("-");
        let version_display = captured_version.as_deref().unwrap_or("-");
        tracing::info!(
            target: "pnpr::audit",
            action = "package.dist_tag_set",
            actor = %actor,
            package = %raw_name,
            tag = %tag,
            version = %version_display,
            registry = %source,
            "package dist-tag set",
        );
    }
    res
}

pub(crate) async fn remove_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
) -> Response {
    if let Err(err) = validate_dist_tag(tag) {
        return err.into_response();
    }
    let mut removed = false;
    let (res, source) = update_dist_tag(state, identity, registry, raw_name, tag, |tags| {
        removed = tags.remove(tag).is_some();
        Ok(())
    })
    .await;
    if removed && let Some(source) = &source {
        let actor = identity.username().unwrap_or("-");
        tracing::info!(
            target: "pnpr::audit",
            action = "package.dist_tag_remove",
            actor = %actor,
            package = %raw_name,
            tag = %tag,
            registry = %source,
            "package dist-tag removed",
        );
    }
    res
}

/// Shared "read packument, mutate dist-tags, write back" helper for
/// add/remove. Returns 201 on success.
async fn update_dist_tag<Mutate>(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    mut mutate: Mutate,
) -> (Response, Option<String>)
where
    Mutate: FnMut(&mut serde_json::Map<String, Value>) -> Result<(), RegistryError>,
{
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return (err.into_response(), None),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return (err.into_response(), None),
    };
    if let Err(err) = authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
        Some(&target.org),
    )
    .await
    {
        return (err.into_response(), None);
    }
    let org = target.org;
    let storage = hosted_storage(state, Some(&org));

    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let _ = tag;
    let outcome = storage.update_hosted_document_with_retry(
        &name,
        DOCUMENT_WRITE_RETRIES,
        |existing_bytes| retag_packument(existing_bytes, &mut mutate),
    )
    .await;
    match outcome {
        Ok(DocumentUpdate::Written) => (ok_created_response(), Some(target.source)),
        Ok(DocumentUpdate::NotFound) => (not_found(), None),
        Err(err) => (err.into_response(), None),
    }
}

fn ok_created_response() -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static-shape response always builds")
}

/// The stored packument with its `dist-tags` mutated and `time.modified`
/// stamped, or `None` when no packument is stored.
fn retag_packument<Mutate>(
    existing_bytes: Option<&[u8]>,
    mutate: &mut Mutate,
) -> Result<Option<Vec<u8>>, RegistryError>
where
    Mutate: FnMut(&mut serde_json::Map<String, Value>) -> Result<(), RegistryError>,
{
    let Some(bytes) = existing_bytes else {
        return Ok(None);
    };
    let mut packument: Value = serde_json::from_slice(bytes)?;
    if let Some(format_version) = packument.get("formatVersion").and_then(Value::as_u64)
        && format_version > u64::from(pnpr_storage::publish::PACKUMENT_FORMAT_VERSION)
    {
        return Err(RegistryError::DocumentFormatTooNew {
            document: "packument".to_string(),
            format_version: format_version as u32,
            supported_version: pnpr_storage::publish::PACKUMENT_FORMAT_VERSION,
        });
    }
    let Some(packument_obj) = packument.as_object_mut() else {
        return Err(RegistryError::BadRequest {
            reason: "stored packument is not an object".to_string(),
        });
    };
    packument_obj.insert(
        "formatVersion".to_string(),
        serde_json::json!(pnpr_storage::publish::PACKUMENT_FORMAT_VERSION),
    );
    let tags_entry = packument_obj
        .entry("dist-tags".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(tags) = tags_entry.as_object_mut() else {
        return Err(RegistryError::BadRequest {
            reason: "stored dist-tags is not an object".to_string(),
        });
    };
    mutate(tags)?;
    update_modified_time(packument_obj)?;
    Ok(Some(serde_json::to_vec_pretty(&packument)?))
}

fn update_modified_time(
    packument_obj: &mut serde_json::Map<String, Value>,
) -> Result<(), RegistryError> {
    let time_entry = packument_obj
        .entry("time".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(time_obj) = time_entry.as_object_mut() else {
        return Err(RegistryError::BadRequest {
            reason: "stored time is not an object".to_string(),
        });
    };
    time_obj.insert("modified".to_string(), Value::String(now_iso()));
    Ok(())
}
