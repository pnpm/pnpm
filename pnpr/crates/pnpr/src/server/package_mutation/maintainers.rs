use axum::response::{
    IntoResponse,
    Response,
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{
    DOCUMENT_WRITE_RETRIES,
    DocumentWrite,
    wait_after_document_write_conflict,
};
use serde_json::{
    Value,
    json,
};

use super::{
    super::{
        Action,
        AppState,
        WriteTarget,
        hosted_storage,
    },
    read_authorized_packument,
};

#[derive(Clone, Copy)]
pub(in crate::server) enum OwnerEdit<'a> {
    Replace(&'a Value),
    Add(&'a str),
    Remove(&'a str),
}

pub(in crate::server) async fn update_package_maintainers(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    (name, edit): (&CanonicalPackageName, OwnerEdit<'_>),
) -> Response {
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    for attempt in 0..DOCUMENT_WRITE_RETRIES {
        match try_update_package_maintainers(state, identity, target, (name, edit)).await {
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

async fn try_update_package_maintainers(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    (name, edit): (&CanonicalPackageName, OwnerEdit<'_>),
) -> Result<Option<Response>, Response> {
    let Some((mut hosted, version)) =
        read_authorized_packument(state, identity, (target, name), Action::Annotate)
            .await
            .map_err(IntoResponse::into_response)?
    else {
        return Err(RegistryError::NotFound.into_response());
    };
    check_maintainer_authority(state, identity, &hosted, name)
        .map_err(IntoResponse::into_response)?;
    let maintainers = edit.apply(state, name, &hosted).await?;
    if hosted.get("maintainers").and_then(Value::as_array) == Some(&maintainers) {
        return Ok(Some(super::ok_created()));
    }
    hosted["maintainers"] = Value::Array(maintainers);
    update_modified_time(&mut hosted);
    let bytes = serde_json::to_vec_pretty(&hosted)
        .map_err(RegistryError::Json)
        .map_err(axum::response::IntoResponse::into_response)?;
    let storage = hosted_storage(state, Some(&target.namespace));
    match storage
        .write_hosted_document_if_current(name, &bytes, Some(&version), identity.user_id())
        .await
        .map_err(IntoResponse::into_response)?
    {
        DocumentWrite::Written => {
            log_maintainers_updated(identity, name, &target.source);
            Ok(Some(super::ok_created()))
        }
        DocumentWrite::Conflict => Ok(None),
    }
}

impl OwnerEdit<'_> {
    async fn apply(
        self,
        state: &AppState,
        name: &CanonicalPackageName,
        hosted: &Value,
    ) -> Result<Vec<Value>, Response> {
        let current = || {
            hosted["maintainers"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        };
        let maintainers = match self {
            Self::Replace(body) => {
                super::revisions::check(name, hosted, body.get("_rev").and_then(Value::as_str))
                    .map_err(IntoResponse::into_response)?;
                let mut entries = extract_and_validate_maintainers(body)?;
                bind_maintainer_ids(state, hosted, &mut entries).await
                    .map_err(IntoResponse::into_response)?;
                entries
            }
            Self::Add(user) => {
                let id = maintainer_id(state, user, None, None).await
                    .map_err(IntoResponse::into_response)?;
                let mut entries = current();
                if let Some(entry) = entries
                    .iter_mut()
                    .find(|entry| entry["name"] == user)
                {
                    entry["userId"] = json!(id);
                } else {
                    entries.push(json!({"name": user, "userId": id}));
                }
                entries
            }
            Self::Remove(user) => {
                let mut entries = current();
                entries.retain(|entry| entry["name"] != user);
                entries
            }
        };
        if maintainers.is_empty() {
            return Err(RegistryError::EmptyMaintainers { package: name.as_str().to_string() }
                .into_response());
        }
        Ok(maintainers)
    }
}

fn extract_and_validate_maintainers(body: &Value) -> Result<Vec<Value>, Response> {
    let Some(maintainers) = body.get("maintainers").and_then(Value::as_array) else {
        return Err(RegistryError::BadRequest {
            reason: "maintainers must be an array".to_string(),
        }
        .into_response());
    };
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
    let actor = crate::audit::Actor::of(identity);
    crate::audit!(
        actor,
        "package.update_maintainers",
        package = %name.as_str(),
        registry = %source,
        "package maintainers updated",
    );
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
        time_obj.insert(
            "modified".to_string(),
            json!(pnpm_time::format_rfc3339(jiff::Timestamp::now())),
        );
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
            if users.active_account_name(id).await?.as_deref() != Some(name) {
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
                .active_account_id(name)
                .await?
                .ok_or_else(|| RegistryError::BadRequest {
                    reason: format!("unknown maintainer {name:?}"),
                }),
        },
    }
}
