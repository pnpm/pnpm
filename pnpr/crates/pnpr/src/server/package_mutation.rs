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

/// Hold a published version's security-critical `dist` fields immutable across
/// the partial-unpublish `PUT`, which otherwise persists the body verbatim.
/// [`super::expected_tarball_dist`] resolves a tarball request to a version by
/// `dist.tarball` basename and verifies the bytes against that version's string
/// `dist.integrity`, so letting either drift — while the bytes on disk stay put —
/// breaks installs of that version (`EINTEGRITY`, or a 404/502 redirect).
///
/// For each version in the body, given a hosted packument: changing the
/// `dist.integrity` or `dist.tarball` basename of an already-published version is
/// rejected; omitting either is repaired from the hosted value (the round-trip
/// drops them on retained versions); and a version not already published is
/// rejected — this endpoint only removes versions, and an added entry could
/// collide a basename or seed a tarball-less one. A `PUT` to a package with no
/// hosted packument is rejected outright (nothing to unpublish, and the write
/// would seed versions that publish can never overwrite).
///
/// Returns the rejection, or `None` when the body is acceptable (after any
/// restores). Must hold the package lock so a concurrent publish can't race it.
fn enforce_published_version_immutability(
    hosted: &Value,
    name: &CanonicalPackageName,
    incoming: &mut Value,
) -> Option<RegistryError> {
    // None (no versions to enforce) means "accept", not "error" here.
    let incoming_versions = incoming.get("versions").and_then(Value::as_object)?;
    let hosted_versions = hosted.get("versions").and_then(Value::as_object);
    // Fields to re-insert after the scan; deferred because the scan borrows
    // `incoming` and the restore mutates it.
    let mut restore: Vec<(String, &'static str, Value)> = Vec::new();
    for (version, manifest) in incoming_versions {
        let Some(existing) = hosted_versions.and_then(|versions| versions.get(version)) else {
            return Some(RegistryError::BadRequest {
                reason: format!(
                    "version {version:?} is not in the published package; this endpoint removes versions, it does not add them",
                ),
            });
        };
        let entry = PublishedVersion { version, existing, manifest };
        if let Some(err) = check_integrity_immutable(&entry, &mut restore) {
            return Some(err);
        }
        if let Some(err) = check_tarball_immutable(&entry, name, &mut restore) {
            return Some(err);
        }
    }
    for (version, key, value) in restore {
        if let Some(dist) = incoming
            .get_mut("versions")
            .and_then(|versions| versions.get_mut(&version))
            .and_then(|manifest| manifest.get_mut("dist"))
            .and_then(Value::as_object_mut)
        {
            dist.insert(key.to_string(), value);
        }
    }
    None
}

/// The packument a rewrite submitted, with the fields only the store owns
/// stripped.
///
/// The write destination is the URL package name; a mismatched body name would
/// otherwise land under the URL package and persist an inconsistent manifest.
fn submitted_packument(body: &[u8], name: &CanonicalPackageName) -> Result<Value, RegistryError> {
    let mut packument: Value = serde_json::from_slice(body).map_err(RegistryError::Json)?;
    if let Some(body_name) = packument.get("name").and_then(Value::as_str)
        && body_name != name.as_str()
    {
        return Err(RegistryError::BadRequest {
            reason: format!(
                "packument name {body_name:?} does not match the URL package {:?}",
                name.as_str(),
            ),
        });
    }
    if let Some(obj) = packument.as_object_mut() {
        obj.remove("_attachments");
        obj.remove("_rev");
        obj.remove("_revisions");
    }
    Ok(packument)
}

/// One version of an update, beside the version the store already holds.
struct PublishedVersion<'a> {
    version: &'a String,
    existing: &'a Value,
    manifest: &'a Value,
}

/// `dist.integrity` of a published version may not change, and an update that
/// omits it has the stored one put back.
fn check_integrity_immutable(
    entry: &PublishedVersion<'_>,
    restore: &mut Vec<(String, &'static str, Value)>,
) -> Option<RegistryError> {
    let version = entry.version;
    // A present dist.integrity must be a string; a non-string would slip past
    // the string-only checks below.
    let incoming = match entry.manifest.get("dist").and_then(|dist| dist.get("integrity")) {
        None => None,
        Some(Value::String(value)) => Some(value.as_str()),
        Some(_) => {
            return Some(RegistryError::BadRequest {
                reason: format!("dist.integrity for version {version:?} must be a string"),
            });
        }
    };
    let stored = entry
        .existing
        .get("dist")
        .and_then(|dist| dist.get("integrity"))
        .and_then(Value::as_str)?;
    match incoming {
        Some(submitted) if submitted != stored => Some(RegistryError::BadRequest {
            reason: format!("dist.integrity for the published version {version:?} is immutable"),
        }),
        Some(_) => None,
        None => {
            let refusal = require_object_dist(entry.manifest, version);
            if refusal.is_none() {
                restore.push((version.clone(), "integrity", Value::String(stored.to_string())));
            }
            refusal
        }
    }
}

/// `dist.tarball` of a published version may not change, and an update that
/// omits it has the stored one put back.
///
/// Basenames are compared, not URLs: the round-trip carries the rewritten URL
/// (see [`pnpr_upstream::rewrite_tarball_urls`]) while the hosted side keeps
/// the original, and [`served_tarball_basename`] applies the same
/// version-derived fallback so a basename-less stored URL is still pinned.
fn check_tarball_immutable(
    entry: &PublishedVersion<'_>,
    name: &CanonicalPackageName,
    restore: &mut Vec<(String, &'static str, Value)>,
) -> Option<RegistryError> {
    let version = entry.version;
    let stored_basename = served_tarball_basename(entry.existing, name)?;
    let incoming_basename = entry
        .manifest
        .get("dist")
        .and_then(|dist| dist.get("tarball"))
        .and_then(Value::as_str)
        .and_then(tarball_basename);
    match incoming_basename {
        Some(submitted) if submitted != stored_basename => Some(RegistryError::BadRequest {
            reason: format!("dist.tarball for the published version {version:?} is immutable"),
        }),
        Some(_) => None,
        None => {
            let refusal = require_object_dist(entry.manifest, version);
            if refusal.is_none() {
                let stored = entry
                    .existing
                    .get("dist")
                    .and_then(|dist| dist.get("tarball"))
                    .cloned()
                    .unwrap_or(Value::Null);
                restore.push((version.clone(), "tarball", stored));
            }
            refusal
        }
    }
}

/// The tarball basename a version is actually served under, mirroring
/// [`pnpr_upstream::rewrite_tarball_urls`]: the `dist.tarball` URL's own basename when it has
/// one, otherwise the version-derived canonical name the rewrite falls back to.
/// Returns `None` when the manifest carries no string `dist.tarball` to serve.
fn served_tarball_basename(manifest: &Value, pkg: &CanonicalPackageName) -> Option<String> {
    let url = manifest.get("dist").and_then(|dist| dist.get("tarball")).and_then(Value::as_str)?;
    if let Some(basename) = tarball_basename(url) {
        return Some(basename.to_owned());
    }
    let version = manifest.get("version").and_then(Value::as_str)?;
    Some(pkg.tarball_name_for_version(version))
}

/// Reject a published version whose `dist` isn't an object: a restore needs an
/// object to write into, so otherwise it would no-op and persist the version
/// without the field — the stripping this guards against.
fn require_object_dist(manifest: &Value, version: &str) -> Option<RegistryError> {
    if manifest.get("dist").is_some_and(Value::is_object) {
        return None;
    }
    Some(RegistryError::BadRequest {
        reason: format!("dist for the published version {version:?} must be an object"),
    })
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
