pub(super) use dist_tags::{get_dist_tags, remove_dist_tag, set_dist_tag};

mod dist_tags;
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
use pnpr_storage::DocumentWrite;

use super::{
    Action, AppState, RegistrySource, authorize, hosted_storage, resolve_write_target,
    tarball_basename,
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
    rewrite_packument(state, identity, &target.source, &storage, &name, &mut packument).await
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
    let written =
        storage.write_hosted_document_if_current(name, &bytes, Some(&hosted_packument.version))
            .await;
    match written {
        Ok(DocumentWrite::Written) => {
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
pub(super) fn authorize_rewrite(
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
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    if let Err(err) = hosted_storage(state, Some(&org)).remove_package(&name).await {
        return err.into_response();
    }
    log_package_deleted(identity, &name, &target.source);
    ok_created_response()
}

fn log_package_deleted(identity: &Identity, name: &CanonicalPackageName, source: &str) {
    let actor = identity.username().unwrap_or("-");
    tracing::info!(
        target: "pnpr::audit",
        action = "package.unpublish",
        actor = %actor,
        package = %name.as_str(),
        registry = %source,
        "entire package unpublished",
    );
}

/// `DELETE /:pkg/-/:filename/-rev/:rev` — remove a single tarball
/// file from the package directory. The partial-unpublish flow calls
/// this after PUT'ing the modified packument back. Accept the
/// libnpmpublish-style scoped filename as well as the canonical one
/// by going through `parse_tarball_name` first.
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
    let (canonical, version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
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
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    if let Err(err) = hosted_storage(state, Some(&org)).remove_blob(&name, &canonical).await {
        return err.into_response();
    }
    let sigstore_name = name.sigstore_name_for_version(&version);
    let _ = hosted_storage(state, Some(&org)).remove_blob(&name, &sigstore_name).await;
    log_tarball_deleted(identity, &name, &canonical, &version, &target.source);
    ok_created_response()
}

fn log_tarball_deleted(
    identity: &Identity,
    name: &CanonicalPackageName,
    filename: &str,
    version: &str,
    source: &str,
) {
    let actor = identity.username().unwrap_or("-");
    tracing::info!(
        target: "pnpr::audit",
        action = "package.unpublish_tarball",
        actor = %actor,
        package = %name.as_str(),
        tarball = %filename,
        version = %version,
        registry = %source,
        "package tarball unpublished",
    );
}

fn ok_created_response() -> Response {
    let body = json!({ "ok": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}
