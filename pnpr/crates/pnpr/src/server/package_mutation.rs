pub(super) mod revisions;

pub(super) use dist_tags::{get_dist_tags, remove_dist_tag, set_dist_tag};
pub(super) use unpublish::{delete_package, delete_tarball};

mod dist_tags;
mod immutability;
mod maintainers;
mod unpublish;
mod version_removal;

#[cfg(test)]
mod tests;

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
use pnpr_storage::{DocumentWrite, PackageTombstone};

use super::{
    Action, AppState, RegistrySource, authorize_package, hosted_storage, resolve_write_target,
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
    let source = RegistrySource::Hosted(target.source.clone());
    let org = target.org;
    let storage = hosted_storage(state, Some(&org));

    let incoming_val: Value = match serde_json::from_slice(body) {
        Ok(val) => val,
        Err(err) => return RegistryError::Json(err).into_response(),
    };

    if incoming_val.get("maintainers").is_some() && incoming_val.get("versions").is_none() {
        if let Err(err) =
            authorize_package(state, identity, &source, name.as_str(), Action::Publish, Some(&org))
                .await
        {
            return err.into_response();
        }
        return update_package_maintainers(
            state,
            identity,
            &target.source,
            &storage,
            &name,
            &incoming_val,
        )
        .await;
    }

    if let Err(err) = authorize_rewrite(state, identity, &source, name.as_str(), Some(&org)).await {
        return err.into_response();
    }
    let mut packument = match submitted_packument(incoming_val, (&name, revision)) {
        Ok(packument) => packument,
        Err(err) => return err.into_response(),
    };
    rewrite_packument(state, identity, &target.source, &storage, &name, &mut packument).await
}

async fn read_hosted_packument(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
) -> Result<pnpr_storage::HostedDocumentForUpdate, Response> {
    match storage.read_hosted_document_for_update(name).await {
        Ok(Some(packument)) => Ok(packument),
        Ok(None) => Err(no_published_packument(name).into_response()),
        Err(err) => Err(err.into_response()),
    }
}

fn prepare_packument_rewrite(
    hosted_bytes: &[u8],
    tar_integrities: &std::collections::BTreeMap<String, pnpr_storage::DerivedTar>,
    name: &CanonicalPackageName,
    packument: &mut Value,
) -> Result<(Value, Vec<String>), Response> {
    let mut hosted: Value = serde_json::from_slice(hosted_bytes)
        .map_err(|err| RegistryError::Json(err).into_response())?;
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
    let removed = detect_removed_versions(&hosted, packument);
    Ok((hosted, removed))
}

/// Serialize against other same-package writers so the rewrite cannot
/// interleave with a concurrent publish or dist-tag merge.
async fn rewrite_packument(
    state: &AppState,
    identity: &Identity,
    source: &str,
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    packument: &mut serde_json::Value,
) -> Response {
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let hosted_packument = match read_hosted_packument(storage, name).await {
        Ok(packument) => packument,
        Err(response) => return response,
    };
    let tar_integrities = storage.read_tar_integrities(name).await.unwrap_or_default();
    let (hosted, removed) =
        match prepare_packument_rewrite(&hosted_packument.bytes, &tar_integrities, name, packument)
        {
            Ok(pair) => pair,
            Err(response) => return response,
        };
    let bytes = match serde_json::to_vec_pretty(&packument) {
        Ok(bytes) => bytes,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    // The tombstone names the removed versions before the rewrite removes
    // them: the metadata store may refuse a removed version for good, so a
    // rewrite could not be undone if recording them failed after it.
    if let Err(err) =
        record_removed_versions_in_tombstone(storage, identity, name, &hosted, &removed).await
    {
        return err.into_response();
    }
    let unpublished = unpublished_blobs(storage, name, &removed).await;
    let version = Some(&hosted_packument.version);
    match storage.write_hosted_document_if_current(name, &bytes, version, identity.user_id()).await
    {
        Ok(DocumentWrite::Written) => {
            reclaim_unpublished_blobs(storage, name, unpublished).await;
            log_packument_update(identity, name, source);
            ok_created()
        }
        Ok(DocumentWrite::Conflict) => {
            RegistryError::DocumentWriteConflict { package: name.as_str().to_string() }
                .into_response()
        }
        Err(err) => err.into_response(),
    }
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

async fn record_removed_versions_in_tombstone(
    storage: &pnpr_storage::Storage,
    identity: &Identity,
    name: &CanonicalPackageName,
    hosted: &Value,
    removed_versions: &[String],
) -> Result<(), RegistryError> {
    if removed_versions.is_empty() {
        return Ok(());
    }
    let fallback = new_tombstone_from_hosted(identity, name, hosted);
    let removed = removed_versions.to_vec();
    storage.update_tombstone(name, move |existing| {
        let mut tombstone = existing.unwrap_or_else(|| fallback.clone());
        for version in &removed {
            if !tombstone.unpublished_versions
                .iter()
                .any(|existing| existing == version)
            {
                tombstone.unpublished_versions.push(version.clone());
            }
        }
        tombstone
    })
    .await?;
    Ok(())
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

fn new_tombstone_from_hosted(
    identity: &Identity,
    name: &CanonicalPackageName,
    hosted: &Value,
) -> PackageTombstone {
    let mut maintainers = Vec::new();
    unpublish::collect_maintainers_from_value(hosted, &mut maintainers);
    PackageTombstone {
        package: name.as_str().to_string(),
        unpublished_by: identity.user_id().map(ToString::to_string),
        unpublished_at: chrono::Utc::now().to_rfc3339(),
        maintainers,
        unpublished_versions: Vec::new(),
    }
}

fn log_packument_update(identity: &Identity, name: &CanonicalPackageName, source: &str) {
    let actor = identity.username().unwrap_or("-");
    tracing::info!(
        target: "pnpr::audit",
        action = "package.update_packument",
        actor = %actor,
        package = %name.as_str(),
        registry = %source,
        "package packument updated",
    );
}

/// A packument rewrite adds nothing but may remove anything, so it is held to
/// both write permissions.
pub(super) async fn authorize_rewrite(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    name: &str,
    org: Option<&str>,
) -> Result<(), RegistryError> {
    for action in [Action::Publish, Action::Unpublish] {
        authorize_package(state, identity, source, name, action, org).await?;
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
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static-shape response always builds")
}
