use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;

use super::super::{
    Action, AppState, RegistrySource, WriteTarget, authorize_package, hosted_storage,
    resolve_write_target,
};

pub(crate) async fn delete_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_and_authorize_unpublish(state, identity, registry, &name).await {
        Ok(target) => target,
        Err(response) => return response,
    };
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let storage = hosted_storage(state, Some(&target.namespace));

    let removed = match storage.remove_package(&name, identity.user_id()).await {
        Ok(removed) => removed,
        Err(err) => return err.into_response(),
    };
    if removed {
        log_package_deleted(identity, &name, &target.source);
    }
    ok_created_response()
}

pub(crate) async fn delete_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    let (canonical, version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    if !super::super::publishing::is_valid_semver(&version) {
        return RegistryError::BadRequest {
            reason: format!("version {version:?} is not valid semver"),
        }
        .into_response();
    }
    let target = match resolve_and_authorize_unpublish(state, identity, registry, &name).await {
        Ok(target) => target,
        Err(response) => return response,
    };
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let storage = hosted_storage(state, Some(&target.namespace));
    match package_exists(&storage, &name).await {
        Ok(true) => {}
        Ok(false) => return RegistryError::NotFound.into_response(),
        Err(err) => return err.into_response(),
    }
    match super::version_removal::remove_listed_version(&storage, identity, &name, &version).await {
        Ok(true) => log_tarball_deleted(identity, &name, &canonical, &version, &target.source),
        Ok(false) => {}
        Err(err) => return err.into_response(),
    }
    ok_created_response()
}

async fn resolve_and_authorize_unpublish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &CanonicalPackageName,
) -> Result<WriteTarget, Response> {
    let target = match resolve_write_target(state, identity, registry, name) {
        Ok(target) => target,
        Err(err) => return Err(err.into_response()),
    };
    if let Err(err) = authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Unpublish,
        Some(&target.namespace),
    )
    .await
    {
        return Err(err.into_response());
    }
    Ok(target)
}

async fn package_exists(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
) -> Result<bool, RegistryError> {
    if storage.read_hosted_document(name).await?.is_some() {
        return Ok(true);
    }
    Ok(storage.read_tombstone(name).await?.is_some())
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
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static-shape response always builds")
}
