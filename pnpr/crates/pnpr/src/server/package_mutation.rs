pub(super) mod revisions;

pub(super) use dist_tags::{get_dist_tags, remove_dist_tag, set_dist_tag};
pub(super) use unpublish::{delete_package, delete_tarball};

mod dist_tags;
mod immutability;
mod maintainers;
mod unpublish;
mod version_removal;

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;

use immutability::{enforce_published_version_immutability, submitted_packument};
use maintainers::update_package_maintainers;
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{
    DOCUMENT_WRITE_RETRIES, DocumentWrite, HostedDocumentVersion,
    metadata::validate_document_dist_tags, wait_after_document_write_conflict,
};

use super::{
    Action, AppState, RegistrySource, WriteTarget, authorize_against_packument, authorize_package,
    hosted_storage, parse_json_body, resolve_write_target,
};

/// Apply a partial unpublish based on its revision, or an owner update.
pub(super) async fn update_packument(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    (raw_name, revision): (&str, &str),
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
    let incoming_val: Value = match parse_json_body(body) {
        Ok(val) => val,
        Err(err) => return err.into_response(),
    };
    if incoming_val.get("maintainers").is_some() && incoming_val.get("versions").is_none() {
        return update_package_maintainers(state, identity, &target, (&name, &incoming_val)).await;
    }
    rewrite_packument(state, identity, &target, (&name, revision, &incoming_val)).await
}

fn prepare_packument_rewrite(
    mut hosted: Value,
    tar_integrities: &std::collections::BTreeMap<String, pnpr_storage::DerivedTar>,
    name: &CanonicalPackageName,
    packument: &mut Value,
) -> Result<Vec<String>, Response> {
    if let Some(format_version) = hosted.get("formatVersion").and_then(Value::as_u64)
        && format_version > u64::from(pnpr_storage::publish::PACKUMENT_FORMAT_VERSION)
    {
        return Err(RegistryError::DocumentFormatTooNew {
            document: name.as_str().to_string(),
            format_version: format_version as u32,
            supported_version: pnpr_storage::publish::PACKUMENT_FORMAT_VERSION,
        }
        .into_response());
    }
    revisions::check(name, &hosted, packument.get("_rev").and_then(Value::as_str))
        .map_err(IntoResponse::into_response)?;
    packument
        .as_object_mut()
        .expect("submitted packument is an object")
        .remove("_rev");
    super::tar_registry::overlay_verified_tar_stats(&mut hosted, tar_integrities);
    super::restore_tgz_integrities(&hosted, tar_integrities, packument);
    if let Some(err) = enforce_published_version_immutability(&hosted, name, packument) {
        return Err(err.into_response());
    }
    preserve_existing_maintainers(&hosted, packument);
    super::publishing::variants::restore_server_fields(&hosted, packument);
    if let Some(packument_obj) = packument.as_object_mut() {
        packument_obj.insert(
            "formatVersion".to_string(),
            serde_json::json!(pnpr_storage::publish::PACKUMENT_FORMAT_VERSION),
        );
    }
    validate_document_dist_tags(&hosted, packument).map_err(IntoResponse::into_response)?;
    Ok(detect_removed_versions(&hosted, packument))
}

/// Serialize against other same-package writers so the rewrite cannot
/// interleave with a concurrent publish or dist-tag merge.
async fn rewrite_packument(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    (name, revision, incoming_val): (&CanonicalPackageName, &str, &Value),
) -> Response {
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    for attempt in 0..DOCUMENT_WRITE_RETRIES {
        match try_rewrite_packument(state, identity, target, (name, revision, incoming_val)).await {
            Ok(Some(response)) => return response,
            Ok(None) if attempt + 1 < DOCUMENT_WRITE_RETRIES => {
                wait_after_document_write_conflict(attempt).await;
            }
            Ok(None) => {}
            Err(response) => return response,
        }
    }
    RegistryError::DocumentWriteConflict { package: name.as_str().to_string() }.into_response()
}

async fn try_rewrite_packument(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    (name, revision, incoming_val): (&CanonicalPackageName, &str, &Value),
) -> Result<Option<Response>, Response> {
    let (hosted, version) = read_packument_for_rewrite(state, identity, target, name).await?;
    let mut packument = submitted_packument(incoming_val.clone(), (name, revision))
        .map_err(IntoResponse::into_response)?;
    let storage = hosted_storage(state, Some(&target.namespace));
    let tar_integrities = storage.read_tar_integrities(name).await.unwrap_or_default();
    let removed = prepare_packument_rewrite(hosted, &tar_integrities, name, &mut packument)?;
    let bytes = serde_json::to_vec_pretty(&packument)
        .map_err(|err| RegistryError::Json(err).into_response())?;
    let unpublished = unpublished_blobs(&storage, name, &removed).await;
    match storage
        .write_hosted_document_if_current(name, &bytes, Some(&version), identity.user_id())
        .await
        .map_err(IntoResponse::into_response)?
    {
        DocumentWrite::Written => {
            reclaim_unpublished_blobs(&storage, name, unpublished).await;
            log_packument_update(identity, name, &target.source);
            Ok(Some(ok_created()))
        }
        DocumentWrite::Conflict => Ok(None),
    }
}

async fn read_packument_for_rewrite(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    name: &CanonicalPackageName,
) -> Result<(Value, HostedDocumentVersion), Response> {
    let registry_source = RegistrySource::Hosted(target.source.clone());
    let Some((hosted, version)) =
        read_authorized_packument(state, identity, (target, name), Action::Publish)
            .await
            .map_err(IntoResponse::into_response)?
    else {
        authorize_package(
            state,
            identity,
            &registry_source,
            name.as_str(),
            Action::Unpublish,
            Some(&target.namespace),
        )
        .await
        .map_err(IntoResponse::into_response)?;
        return Err(no_published_packument(name).into_response());
    };
    authorize_against_packument(
        state,
        identity,
        &registry_source,
        (name.as_str(), &hosted),
        Action::Unpublish,
    )
    .map_err(IntoResponse::into_response)?;
    Ok((hosted, version))
}

pub(super) async fn read_authorized_packument(
    state: &AppState,
    identity: &Identity,
    (target, name): (&WriteTarget, &CanonicalPackageName),
    action: Action,
) -> Result<Option<(Value, HostedDocumentVersion)>, RegistryError> {
    let storage = hosted_storage(state, Some(&target.namespace));
    let source = RegistrySource::Hosted(target.source.clone());
    let Some(stored) = storage.read_hosted_document_for_update(name).await? else {
        authorize_package(state, identity, &source, name.as_str(), action, Some(&target.namespace))
            .await?;
        return Ok(None);
    };
    let packument: Value = serde_json::from_slice(&stored.bytes)?;
    authorize_against_packument(state, identity, &source, (name.as_str(), &packument), action)?;
    Ok(Some((packument, stored.version)))
}

fn preserve_existing_maintainers(hosted: &Value, packument: &mut Value) {
    let Some(obj) = packument.as_object_mut() else { return };
    if let Some(existing) = hosted.get("maintainers") {
        obj.insert("maintainers".to_string(), existing.clone());
    } else {
        obj.remove("maintainers");
    }
}

fn detect_removed_versions(hosted: &Value, packument: &Value) -> Vec<String> {
    let Some(hosted_versions) = hosted.get("versions").and_then(Value::as_object) else {
        return Vec::new();
    };
    let incoming_versions = packument.get("versions").and_then(Value::as_object);
    hosted_versions
        .keys()
        .filter(|version| incoming_versions.is_none_or(|inc| !inc.contains_key(*version)))
        .cloned()
        .collect()
}

/// The blobs the files of `removed_versions` are mapped to, read before the
/// rewrite drops the versions and their mappings. A failure only warns: the
/// blobs are then left for `pnpr gc`.
async fn unpublished_blobs(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    removed_versions: &[String],
) -> Vec<pnpr_storage::BlobId> {
    match storage.version_blobs(name, removed_versions).await {
        Ok(ids) => ids,
        Err(err) => {
            tracing::warn!(package = %name.as_str(), %err, "unpublished archives left for `pnpr gc`");
            Vec::new()
        }
    }
}

/// A rewrite that drops versions reclaims their archives too: a client that
/// read the packument from a tar registry has no `dist.tarball` to send the
/// follow-up tarball DELETE for. Bytes another file still maps stay. The
/// versions are already unpublished, so an archive that fails to go is
/// logged and left for `pnpr gc`.
async fn reclaim_unpublished_blobs(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    ids: Vec<pnpr_storage::BlobId>,
) {
    if let Err(err) = storage.reclaim_blobs(ids).await {
        tracing::warn!(package = %name.as_str(), %err, "unpublished archives left for `pnpr gc`");
    }
}

fn log_packument_update(identity: &Identity, name: &CanonicalPackageName, source: &str) {
    let actor = crate::audit::Actor::of(identity);
    crate::audit!(
        actor,
        "package.update_packument",
        package = %name.as_str(),
        registry = %source,
        "package packument updated",
    );
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
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static-shape response always builds")
}
