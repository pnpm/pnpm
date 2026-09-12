mod immutability;
use immutability::{enforce_published_version_immutability, submitted_packument};

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate, DocumentWrite, publish::now_iso};

use pnpr_upstream::tarball_basename;

use super::{
    Action, AppState, RegistrySource, authorize, filter_osv_vulnerable_dist_tags, hosted_storage,
    load_packument_for_read, not_found, resolve_write_target,
};

/// `PUT /:pkg/-rev/:rev` (path-less) or `PUT /~<name>/:pkg/-rev/:rev` —
/// overwrite the on-disk packument with the client-supplied body. pnpm uses
/// this in the partial-unpublish flow: it fetches the packument, removes the
/// unpublished version from `versions` / `dist-tags`, then PUTs the result
/// back. We strip any `_attachments` so we don't persist base64 payloads
/// alongside the manifest, and run
/// [`enforce_published_version_immutability`] so the body can't tamper with
/// a published version's `dist` or smuggle in a new one — everything else in
/// the body is trusted verbatim, the same trust verdaccio extends.
pub(super) async fn update_packument(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    body: &[u8],
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    let source = RegistrySource::Hosted(target.source.clone());
    if let Err(err) = authorize_rewrite(state, identity, &source, name.as_str()) {
        return err.into_response();
    }
    let org = target.org;
    let storage = hosted_storage(state, Some(&org));
    let mut packument = match submitted_packument(body, &name) {
        Ok(packument) => packument,
        Err(err) => return err.into_response(),
    };
    rewrite_packument(state, &storage, &name, &mut packument).await
}

/// Serialize against other same-package writers so the rewrite cannot
/// interleave with a concurrent publish or dist-tag merge.
async fn rewrite_packument(
    state: &AppState,
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    packument: &mut serde_json::Value,
) -> Response {
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    let hosted_packument = match storage.read_hosted_document_for_update(name).await {
        Ok(Some(packument)) => packument,
        Ok(None) => return no_published_packument(name).into_response(),
        Err(err) => return err.into_response(),
    };
    let hosted: Value = match serde_json::from_slice(&hosted_packument.bytes) {
        Ok(value) => value,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    if let Some(err) = enforce_published_version_immutability(&hosted, name, packument) {
        return err.into_response();
    }
    let bytes = match serde_json::to_vec_pretty(&packument) {
        Ok(bytes) => bytes,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let written = storage
        .write_hosted_document_if_current(name, &bytes, Some(&hosted_packument.version))
        .await;
    match written {
        Ok(DocumentWrite::Written) => ok_created(),
        Ok(DocumentWrite::Conflict) => {
            RegistryError::DocumentWriteConflict { package: name.as_str().to_string() }
                .into_response()
        }
        Err(err) => err.into_response(),
    }
}

/// A packument rewrite adds nothing but may remove anything, so it is held to
/// both write permissions.
fn authorize_rewrite(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    name: &str,
) -> Result<(), RegistryError> {
    for action in [Action::Publish, Action::Unpublish] {
        authorize(state, identity, source, name, action)?;
    }
    Ok(())
}

fn no_published_packument(name: &CanonicalPackageName) -> RegistryError {
    RegistryError::BadRequest {
        reason: format!(
            "cannot update {:?}: it has no published packument to unpublish from",
            name.as_str(),
        ),
    }
}

fn ok_created() -> Response {
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `DELETE /:pkg/-rev/:rev` (path-less) or `DELETE /~<name>/:pkg/-rev/:rev`
/// — remove the entire package directory, packument and all tarballs. Used
/// by `pnpm unpublish --force`.
pub(super) async fn delete_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Unpublish,
    ) {
        return err.into_response();
    }
    let org = target.org;
    // Serialize against same-package publishers so a delete can't race a
    // stage-and-commit and remove the package mid-write.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = hosted_storage(state, Some(&org)).remove_package(&name).await {
        return err.into_response();
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `DELETE /:pkg/-/:filename/-rev/:rev` — remove a single tarball
/// file from the package directory. The partial-unpublish flow calls
/// this after PUT'ing the modified packument back. Accept the
/// libnpmpublish-style scoped filename as well as the canonical one
/// by going through `canonicalize_tarball_name` first.
pub(super) async fn delete_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };
    let canonical = match name.canonicalize_tarball_name(filename) {
        Ok(c) => c,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Unpublish,
    ) {
        return err.into_response();
    }
    let org = target.org;
    // Serialize against same-package publishers so a delete can't race a
    // stage-and-commit and remove a tarball mid-write.
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;
    if let Err(err) = hosted_storage(state, Some(&org)).remove_blob(&name, &canonical).await {
        return err.into_response();
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// `GET /-/package/:pkg/dist-tags` (path-less) or
/// `GET /~<name>/-/package/:pkg/dist-tags` — return the packument's
/// `dist-tags` object.
pub(super) async fn get_dist_tags(
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
    let mut tags = packument.get("dist-tags").cloned().unwrap_or_else(|| json!({}));
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
/// a JSON-encoded version string (e.g. `"1.0.0"`).
pub(super) async fn set_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    body: &[u8],
) -> Response {
    let mut parsed_version: Option<String> = None;
    update_dist_tag(state, identity, registry, raw_name, tag, move |tags| {
        let version = if let Some(version) = parsed_version.as_ref() {
            version.clone()
        } else {
            let version: String = serde_json::from_slice(body).map_err(RegistryError::Json)?;
            parsed_version = Some(version.clone());
            version
        };
        tags.insert(tag.to_string(), Value::String(version));
        Ok(())
    })
    .await
}

pub(super) async fn remove_dist_tag(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
) -> Response {
    update_dist_tag(state, identity, registry, raw_name, tag, |tags| {
        tags.remove(tag);
        Ok(())
    })
    .await
}

/// Shared "read packument, mutate dist-tags, write back" helper for
/// add/remove. Returns 201 on success — verdaccio uses 201 for both
/// add and remove and the anonymous-npm-registry-client tolerates
/// 200 or 201, so we standardize on 201.
async fn update_dist_tag<Mutate>(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    tag: &str,
    mut mutate: Mutate,
) -> Response
where
    Mutate: FnMut(&mut serde_json::Map<String, Value>) -> Result<(), RegistryError>,
{
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    // A dist-tag change is a write, so it routes to a hosted namespace like
    // a publish — a name routed to an upstream is rejected — and the
    // resolved registry's `publish` rule gates it.
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
    ) {
        return err.into_response();
    }
    let org = target.org;
    let storage = hosted_storage(state, Some(&org));

    // Serialize the read-modify-write against other same-package writers
    // on this instance (held until this function returns).
    let _packument_guard = state.inner.package_locks.lock(name.as_str()).await;

    let _ = tag; // the tag name is captured by the `mutate` closure.
    let outcome = storage
        .update_hosted_document_with_retry(&name, DOCUMENT_WRITE_RETRIES, |existing_bytes| {
            retag_packument(existing_bytes, &mut mutate)
        })
        .await;
    match outcome {
        Ok(DocumentUpdate::Written) => {}
        Ok(DocumentUpdate::NotFound) => return not_found(),
        Err(err) => return err.into_response(),
    }
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
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
    // A hosted org has no upstream, so a dist-tag change starts from the
    // org's own packument; a package it does not host can't be tagged.
    let Some(bytes) = existing_bytes else {
        return Ok(None);
    };
    let mut packument: Value = serde_json::from_slice(bytes)?;
    let Some(packument_obj) = packument.as_object_mut() else {
        return Err(RegistryError::BadRequest {
            reason: "stored packument is not an object".to_string(),
        });
    };
    let tags_entry = packument_obj
        .entry("dist-tags".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(tags) = tags_entry.as_object_mut() else {
        return Err(RegistryError::BadRequest {
            reason: "stored dist-tags is not an object".to_string(),
        });
    };
    mutate(tags)?;
    // Refresh `time.modified` so clients do not lag behind a
    // dist-tag change when deciding packument freshness.
    let time_entry = packument_obj
        .entry("time".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(time_obj) = time_entry.as_object_mut() else {
        return Err(RegistryError::BadRequest {
            reason: "stored time is not an object".to_string(),
        });
    };
    time_obj.insert("modified".to_string(), Value::String(now_iso()));
    Ok(Some(serde_json::to_vec_pretty(&packument)?))
}
