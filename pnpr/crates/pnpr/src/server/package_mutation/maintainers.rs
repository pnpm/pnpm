use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::DocumentWrite;
use serde_json::{Value, json};

use super::{super::AppState, no_published_packument};

pub(super) async fn update_package_maintainers(
    state: &AppState,
    identity: &Identity,
    source: &str,
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    body: &Value,
) -> Response {
    let mut new_maintainers = match extract_and_validate_maintainers(body, name) {
        Ok(maintainers) => maintainers,
        Err(response) => return response,
    };

    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let hosted_packument = match storage.read_hosted_document_for_update(name).await {
        Ok(Some(packument)) => packument,
        Ok(None) => return no_published_packument(name).into_response(),
        Err(err) => return err.into_response(),
    };
    let mut hosted: Value = match serde_json::from_slice(&hosted_packument.bytes) {
        Ok(value) => value,
        Err(err) => return RegistryError::Json(err).into_response(),
    };

    if let Err(err) = check_maintainer_authority(state, identity, &hosted, name) {
        return err.into_response();
    }

    if let Err(err) = bind_maintainer_ids(state, &hosted, &mut new_maintainers).await {
        return err.into_response();
    }
    hosted["maintainers"] = Value::Array(new_maintainers);
    update_modified_time(&mut hosted);
    let bytes = match serde_json::to_vec_pretty(&hosted) {
        Ok(bytes) => bytes,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let version = Some(&hosted_packument.version);
    let written =
        storage.write_hosted_document_if_current(name, &bytes, version, identity.user_id()).await;
    match written {
        Ok(DocumentWrite::Written) => {
            log_maintainers_updated(identity, name, source);
            ok_created()
        }
        Ok(DocumentWrite::Conflict) => {
            RegistryError::DocumentWriteConflict { package: name.as_str().to_string() }
                .into_response()
        }
        Err(err) => err.into_response(),
    }
}

fn extract_and_validate_maintainers(
    body: &Value,
    name: &CanonicalPackageName,
) -> Result<Vec<Value>, Response> {
    let Some(maintainers) = body.get("maintainers").and_then(Value::as_array) else {
        return Err(RegistryError::BadRequest {
            reason: "maintainers must be an array".to_string(),
        }
        .into_response());
    };
    if maintainers.is_empty() {
        return Err(
            RegistryError::EmptyMaintainers { package: name.as_str().to_string() }.into_response()
        );
    }
    for entry in maintainers {
        validate_maintainer_entry(entry)?;
    }
    Ok(maintainers.clone())
}

fn validate_maintainer_entry(entry: &Value) -> Result<(), Response> {
    let Some(obj) = entry.as_object() else {
        return Err(RegistryError::BadRequest {
            reason: "maintainer entries must be objects".to_string(),
        }
        .into_response());
    };
    let Some(name_str) = obj.get("name").and_then(Value::as_str) else {
        return Err(RegistryError::BadRequest {
            reason: "maintainer entry must contain a string name".to_string(),
        }
        .into_response());
    };
    if name_str.trim().is_empty() {
        return Err(RegistryError::BadRequest {
            reason: "maintainer name cannot be empty".to_string(),
        }
        .into_response());
    }
    Ok(())
}

fn check_maintainer_authority(
    state: &AppState,
    identity: &Identity,
    hosted: &Value,
    name: &CanonicalPackageName,
) -> Result<(), RegistryError> {
    let not_maintainer = || RegistryError::NotPackageMaintainer {
        action: "modify maintainers of",
        package: name.as_str().to_string(),
    };
    let Some(caller) = identity.user_id() else {
        return Err(not_maintainer());
    };
    let is_admin = state.inner.config.identity.admins.contains(caller);
    let is_maintainer = hosted
        .get("maintainers")
        .and_then(Value::as_array)
        .is_some_and(|arr| {
            arr.iter()
                .any(|entry| entry.get("userId").and_then(Value::as_str) == Some(caller))
        });
    if is_admin || is_maintainer { Ok(()) } else { Err(not_maintainer()) }
}

fn log_maintainers_updated(identity: &Identity, name: &CanonicalPackageName, source: &str) {
    let actor = identity.username().unwrap_or("-");
    tracing::info!(
        target: "pnpr::audit",
        action = "package.update_maintainers",
        actor = %actor,
        package = %name.as_str(),
        registry = %source,
        "package maintainers updated",
    );
}

fn ok_created() -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static response builds")
}

async fn bind_maintainer_ids(
    state: &AppState,
    hosted: &Value,
    maintainers: &mut [Value],
) -> Result<(), RegistryError> {
    let previous = hosted.get("maintainers").and_then(Value::as_array);
    for entry in maintainers {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or(RegistryError::NotFound)?;
        let existing = previous.and_then(|entries| {
            entries
                .iter()
                .find(|old| old.get("name") == entry.get("name"))
        });
        let previous_id = existing
            .and_then(|old| old.get("userId"))
            .and_then(Value::as_str);
        let id = maintainer_id(state, name, entry.get("userId"), previous_id).await?;
        entry["userId"] = Value::String(id);
    }
    Ok(())
}

fn update_modified_time(hosted: &mut Value) {
    if let Some(time_obj) = hosted.get_mut("time").and_then(Value::as_object_mut) {
        time_obj.insert("modified".to_string(), json!(chrono::Utc::now().to_rfc3339()));
    }
}

async fn maintainer_id(
    state: &AppState,
    name: &str,
    requested: Option<&Value>,
    previous: Option<&str>,
) -> Result<String, RegistryError> {
    let users = &state.inner.identity.auth.users;
    match requested {
        Some(Value::String(id)) if Some(id.as_str()) == previous => Ok(id.clone()),
        Some(Value::String(id)) => {
            if users.account_name(id).await?.as_deref() != Some(name) {
                return Err(RegistryError::BadRequest {
                    reason: format!("maintainer account ID does not match {name:?}"),
                });
            }
            Ok(id.clone())
        }
        Some(_) => Err(RegistryError::BadRequest {
            reason: "maintainer userId must be a string".to_string(),
        }),
        None => match previous {
            Some(id) => Ok(id.to_string()),
            None => users
                .account_id(name)
                .await?
                .ok_or_else(|| RegistryError::BadRequest {
                    reason: format!("unknown maintainer {name:?}"),
                }),
        },
    }
}
