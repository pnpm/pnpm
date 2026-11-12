//! Owner management for hosted crates, Python projects, and image
//! repositories: the accounts a document's `owners` records, which the
//! `$owners` rule token admits.
//!
//! The wire format is the crates.io owners API that `cargo owner` speaks. The
//! Python and OCI surfaces answer the same requests as a pnpr extension, since
//! neither protocol defines one.

use super::{
    AppState, AuthedCaller, RegistrySource, TargetRegistry,
    documents::{HostedDocument, read_hosted_document},
    ecosystem::addressed_registry,
    private_no_cache, resolve_ecosystem_source, resolve_write_target_for,
};
use axum::{
    body::Bytes,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{MethodRouter, get},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// The owner requests of a surface whose errors are pnpr's own JSON. The
/// route captures the package name as `name`.
pub(super) fn owner_routes<Document: HostedDocument + Send + 'static>() -> MethodRouter<AppState> {
    get(list::<Document>).put(add::<Document>).delete(remove::<Document>)
}

async fn list<Document: HostedDocument>(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let listed =
        list_owners::<Document>(&state, &identity, registry.as_deref(), params.get("name")).await;
    private_no_cache(listed.map(axum::Json).into_response())
}

async fn add<Document: HostedDocument>(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let request = OwnerRequest { registry: registry.as_deref(), name: params.get("name"), body };
    let changed = change_owners::<Document>(&state, &identity, request, OwnerChange::Add).await;
    private_no_cache(changed.map(axum::Json).into_response())
}

async fn remove<Document: HostedDocument>(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let request = OwnerRequest { registry: registry.as_deref(), name: params.get("name"), body };
    let changed = change_owners::<Document>(&state, &identity, request, OwnerChange::Remove).await;
    private_no_cache(changed.map(axum::Json).into_response())
}

/// `{"users": [{"id": 1, "login": "alice", "name": "alice"}]}`. An owner whose
/// account no longer exists is listed by its account ID, which is also what
/// removes it.
pub(super) async fn list_owners<Document: HostedDocument>(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: Option<&String>,
) -> Result<Value, RegistryError> {
    let key = parse_name::<Document>(name)?;
    let target =
        addressed_registry(state, registry, Document::ECOSYSTEM).ok_or(RegistryError::NotFound)?;
    let RegistrySource::Hosted(source) =
        resolve_ecosystem_source(state, &target, Document::ECOSYSTEM, key.as_str())
    else {
        return Err(RegistryError::NotFound);
    };
    let document = read_hosted_document::<Document>(state, identity, &source, &key)
        .await?
        .ok_or(RegistryError::NotFound)?;
    let mut users = Vec::with_capacity(document.owners().len());
    for (position, id) in document.owners().iter().enumerate() {
        let login = state.inner.identity.auth.users
            .active_account_name(id)
            .await?
            .unwrap_or_else(|| id.clone());
        // Cargo requires a numeric `id` and only displays `login` and `name`.
        users.push(json!({ "id": position + 1, "login": login, "name": login }));
    }
    Ok(json!({ "users": users }))
}

/// An owner change as the request carries it.
pub(super) struct OwnerRequest<'request> {
    pub(super) registry: Option<&'request str>,
    pub(super) name: Option<&'request String>,
    /// `{"users": ["alice", "bob"]}`.
    pub(super) body: Bytes,
}

#[derive(Clone, Copy)]
pub(super) enum OwnerChange {
    Add,
    Remove,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerLogins {
    users: Vec<String>,
}

/// Add or remove owners, answering `{"ok": true, "msg": ...}`. Only an owner
/// or one of the `auth.admins` may, and the package must keep at least one
/// owner. Adding an owner already listed, or removing one that is not,
/// changes nothing.
pub(super) async fn change_owners<Document: HostedDocument>(
    state: &AppState,
    identity: &Identity,
    request: OwnerRequest<'_>,
    change: OwnerChange,
) -> Result<Value, RegistryError> {
    let key = parse_name::<Document>(request.name)?;
    let logins = parse_logins(&request.body)?;
    let target =
        resolve_write_target_for(state, identity, request.registry, Document::ECOSYSTEM, &key)?;
    let ids = account_ids(state, &logins, change).await?;
    let _guard = state.inner.locks.packages.lock(key.as_str()).await;
    let outcome = state.inner.storage
        .for_hosted(&target.namespace, Document::ECOSYSTEM)
        .update_hosted_document_with_retry(
            &key,
            DOCUMENT_WRITE_RETRIES,
            identity.user_id(),
            |bytes| {
                let Some(bytes) = bytes else { return Ok(None) };
                let mut document = parse_current::<Document>(&key, bytes)?;
                authorize_owner_change(state, identity, &key, document.owners())?;
                apply_change(&key, document.owners_mut(), &ids, change)?;
                Ok(Some(document.to_bytes()))
            },
        )
        .await?;
    if outcome == DocumentUpdate::NotFound {
        return Err(RegistryError::NotFound);
    }
    log_owners_changed(identity, &key, &target.source, change, &logins);
    Ok(json!({ "ok": true, "msg": change_message(&key, change, &logins) }))
}

fn parse_name<Document: HostedDocument>(
    name: Option<&String>,
) -> Result<CanonicalPackageName, RegistryError> {
    let name = name.ok_or(RegistryError::NotFound)?;
    CanonicalPackageName::parse(name, Document::ECOSYSTEM).map_err(|_| RegistryError::NotFound)
}

fn parse_logins(body: &[u8]) -> Result<Vec<String>, RegistryError> {
    let request: OwnerLogins = serde_json::from_slice(body)
        .map_err(|err| RegistryError::BadRequest {
            reason: format!(r#"expected {{"users": [<login>, ...]}}: {err}"#),
        })?;
    if request.users.is_empty() {
        return Err(RegistryError::BadRequest { reason: "users must not be empty".to_string() });
    }
    Ok(request.users)
}

/// The account IDs `logins` name. An owner to add must be an active account;
/// an owner to remove may also be named by its account ID, which is how an
/// owner whose account was deleted is listed.
async fn account_ids(
    state: &AppState,
    logins: &[String],
    change: OwnerChange,
) -> Result<Vec<String>, RegistryError> {
    let users = &state.inner.identity.auth.users;
    let mut ids = Vec::with_capacity(logins.len());
    for login in logins {
        let id = match (users.active_account_id(login).await?, change) {
            (Some(id), _) => id,
            (None, OwnerChange::Remove) => login.clone(),
            (None, OwnerChange::Add) => {
                return Err(RegistryError::BadRequest {
                    reason: format!("unknown user {login:?}"),
                });
            }
        };
        ids.push(id);
    }
    Ok(ids)
}

fn parse_current<Document: HostedDocument>(
    key: &CanonicalPackageName,
    bytes: &[u8],
) -> Result<Document, RegistryError> {
    let document = Document::parse(bytes).map_err(RegistryError::Json)?;
    if document.format_version() > Document::FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.as_str().to_string(),
            format_version: document.format_version(),
            supported_version: Document::FORMAT_VERSION,
        });
    }
    Ok(document)
}

/// Checked against the document being rewritten, so an owner removed by a
/// concurrent write cannot change the owners afterwards.
fn authorize_owner_change(
    state: &AppState,
    identity: &Identity,
    key: &CanonicalPackageName,
    owners: &[String],
) -> Result<(), RegistryError> {
    let allowed = identity
        .user_id()
        .is_some_and(|caller| {
            state.inner.config.identity.admins.contains(caller)
                || owners
                    .iter()
                    .any(|owner| owner == caller)
        });
    if allowed {
        return Ok(());
    }
    Err(RegistryError::Forbidden {
        user: identity
            .username()
            .unwrap_or("anonymous")
            .to_string(),
        action: "change the owners of",
        resource: format!("{:?}", key.as_str()),
    })
}

fn apply_change(
    key: &CanonicalPackageName,
    owners: &mut Vec<String>,
    ids: &[String],
    change: OwnerChange,
) -> Result<(), RegistryError> {
    match change {
        OwnerChange::Add => {
            for id in ids {
                if !owners.contains(id) {
                    owners.push(id.clone());
                }
            }
        }
        OwnerChange::Remove => owners.retain(|owner| !ids.contains(owner)),
    }
    if owners.is_empty() {
        return Err(RegistryError::BadRequest {
            reason: format!("{:?} must keep at least one owner", key.as_str()),
        });
    }
    Ok(())
}

fn change_message(key: &CanonicalPackageName, change: OwnerChange, logins: &[String]) -> String {
    let logins = logins.join(", ");
    match change {
        OwnerChange::Add => format!("added {logins} as owners of {}", key.as_str()),
        OwnerChange::Remove => format!("removed {logins} from the owners of {}", key.as_str()),
    }
}

fn log_owners_changed(
    identity: &Identity,
    key: &CanonicalPackageName,
    source: &str,
    change: OwnerChange,
    logins: &[String],
) {
    let action = match change {
        OwnerChange::Add => "package.add_owners",
        OwnerChange::Remove => "package.remove_owners",
    };
    tracing::info!(
        target: "pnpr::audit",
        action,
        actor = %identity.username().unwrap_or("-"),
        package = %key.as_str(),
        registry = %source,
        owners = %logins.join(","),
        "package owners changed",
    );
}
