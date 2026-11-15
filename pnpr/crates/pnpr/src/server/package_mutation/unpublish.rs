use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, PackageRemoval, wait_after_document_write_conflict};

use super::{
    super::{
        Action, AppState, AuthorizationDecision, RegistrySource, authorize_package,
        authorize_without_owners, hosted_storage, resolve_write_target,
    },
    read_authorized_packument,
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
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;

    for attempt in 0..DOCUMENT_WRITE_RETRIES {
        match try_delete_package(state, identity, &target, &name).await {
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

async fn try_delete_package(
    state: &AppState,
    identity: &Identity,
    target: &super::super::WriteTarget,
    name: &CanonicalPackageName,
) -> Result<Option<Response>, Response> {
    let source = RegistrySource::Hosted(target.source.clone());
    let storage = hosted_storage(state, Some(&target.namespace));
    match authorize_without_owners(state, identity, &source, name.as_str(), Action::Unpublish)
        .map_err(IntoResponse::into_response)?
    {
        AuthorizationDecision::AllowedWithoutOwners => {
            if storage
                .remove_package(name, identity.user_id())
                .await
                .map_err(IntoResponse::into_response)?
            {
                log_package_deleted(identity, name, &target.source);
            }
            return Ok(Some(ok_created_response()));
        }
        AuthorizationDecision::NeedsOwners => {}
    }
    remove_authorized_package(state, identity, target, (&storage, name)).await
}

async fn remove_authorized_package(
    state: &AppState,
    identity: &Identity,
    target: &super::super::WriteTarget,
    (storage, name): (&pnpr_storage::Storage, &CanonicalPackageName),
) -> Result<Option<Response>, Response> {
    let Some((_, version)) =
        read_authorized_packument(state, identity, (target, name), Action::Unpublish)
            .await
            .map_err(IntoResponse::into_response)?
    else {
        return Ok(Some(ok_created_response()));
    };
    match storage
        .remove_package_if_current(name, &version, identity.user_id())
        .await
        .map_err(IntoResponse::into_response)?
    {
        PackageRemoval::Removed => {
            log_package_deleted(identity, name, &target.source);
            Ok(Some(ok_created_response()))
        }
        PackageRemoval::NotFound => {
            let source = RegistrySource::Hosted(target.source.clone());
            authorize_package(
                state,
                identity,
                &source,
                name.as_str(),
                Action::Unpublish,
                Some(&target.namespace),
            )
            .await
            .map_err(IntoResponse::into_response)?;
            Ok(Some(ok_created_response()))
        }
        PackageRemoval::Conflict => Ok(None),
    }
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
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let removed =
        super::version_removal::remove_listed_version(state, identity, &target, (&name, &version))
            .await;
    match removed {
        Ok(true) => log_tarball_deleted(identity, &name, &canonical, &version, &target.source),
        Ok(false) => {}
        Err(err) => return err.into_response(),
    }
    ok_created_response()
}

fn log_package_deleted(identity: &Identity, name: &CanonicalPackageName, source: &str) {
    let actor = crate::audit::Actor::of(identity);
    crate::audit!(
        actor,
        "package.unpublish",
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
    let actor = crate::audit::Actor::of(identity);
    crate::audit!(
        actor,
        "package.unpublish_tarball",
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
