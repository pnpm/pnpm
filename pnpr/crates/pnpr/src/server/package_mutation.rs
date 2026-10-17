pub(super) use dist_tags::{get_dist_tags, remove_dist_tag, set_dist_tag};
pub(super) use unpublish::{delete_package, delete_tarball};

mod dist_tags;
mod immutability;
mod maintainers;
mod unpublish;

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

/// `PUT /:pkg/-rev/:rev` (path-less) or `PUT /~<name>/:pkg/-rev/:rev` —
/// overwrite the on-disk packument with the client-supplied body. pnpm uses
/// this in the partial-unpublish flow: it fetches the packument, removes the
/// unpublished version from `versions` / `dist-tags`, then PUTs the result
/// back. We strip any `_attachments` so we don't persist base64 payloads
/// alongside the manifest, and run
/// [`enforce_published_version_immutability`] so the body can't tamper with
/// a published version's `dist` or smuggle in a new one — everything else in
/// the body is trusted verbatim, the same trust verdaccio extends.
///
/// If the request body contains `maintainers` without `versions`, this is an
/// `npm owner` mutation. Maintainer updates require maintainer/admin privileges
/// and validate that maintainers cannot be emptied.
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
    let mut packument = match submitted_packument(body, &name) {
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
    name: &CanonicalPackageName,
    packument: &mut Value,
) -> Result<(Value, Vec<String>), Response> {
    let hosted: Value = serde_json::from_slice(hosted_bytes)
        .map_err(|err| RegistryError::Json(err).into_response())?;
    if let Some(err) = enforce_published_version_immutability(&hosted, name, packument) {
        return Err(err.into_response());
    }
    preserve_existing_maintainers(&hosted, packument);
    let removed = detect_removed_versions(&hosted, packument);
    Ok((hosted, removed))
}

async fn rollback_hosted_packument(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    written_bytes: &[u8],
    fallback_bytes: &[u8],
) {
    if let Ok(Some(current)) = storage.read_hosted_document_for_update(name).await
        && current.bytes == written_bytes
    {
        let _ =
            storage.write_hosted_document_if_current(name, fallback_bytes, Some(&current.version))
                .await;
    }
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
    let (hosted, removed) =
        match prepare_packument_rewrite(&hosted_packument.bytes, name, packument) {
            Ok(pair) => pair,
            Err(response) => return response,
        };
    let bytes = match serde_json::to_vec_pretty(&packument) {
        Ok(bytes) => bytes,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    match storage.write_hosted_document_if_current(name, &bytes, Some(&hosted_packument.version))
        .await
    {
        Ok(DocumentWrite::Written) => {
            if let Err(err) =
                record_removed_versions_in_tombstone(storage, identity, name, &hosted, &removed)
                    .await
            {
                rollback_hosted_packument(storage, name, &bytes, &hosted_packument.bytes).await;
                return err.into_response();
            }
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

fn new_tombstone_from_hosted(
    identity: &Identity,
    name: &CanonicalPackageName,
    hosted: &Value,
) -> PackageTombstone {
    let mut maintainers = Vec::new();
    if let Some(entries) = hosted.get("maintainers").and_then(Value::as_array) {
        for entry in entries {
            if let Some(user) = entry.get("name").and_then(Value::as_str) {
                maintainers.push(user.to_string());
            }
        }
    }
    PackageTombstone {
        package: name.as_str().to_string(),
        unpublished_by: identity.username().map(ToString::to_string),
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
