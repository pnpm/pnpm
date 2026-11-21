use super::{
    AppState,
    AuthedCaller,
    CanonicalPackageName,
    Deserialize,
    Ecosystem,
    Path,
    Response,
    State,
    TargetRegistry,
    private_no_cache,
    resolve_write_target,
};
use crate::server::{
    HostedGate,
    RegistrySource,
    ecosystem::addressed_registry,
    hosted_gate,
    publishing::registry_visible_to_caller,
    resolve_registry_source,
};
use axum::{
    Json,
    http::Method,
    response::IntoResponse,
};
use pnpr_error::RegistryError;
use pnpr_policy::Identity;

#[derive(Deserialize)]
pub(super) struct StarPackage {
    name: String,
}

#[derive(Deserialize)]
pub(super) struct UserPath {
    user: String,
}

pub(super) async fn set(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    method: Method,
    Json(package): Json<StarPackage>,
) -> Result<Json<serde_json::Value>, RegistryError> {
    let id = account(&identity)?;
    let name = CanonicalPackageName::parse(&package.name, Ecosystem::Npm)?;
    let target = resolve_write_target(&state, &identity, registry.as_deref(), &name)?;
    state.inner.storage
        .sql_database()
        .set_package_star(target.namespace, name.as_str().to_owned(), id, method == Method::PUT)
        .await?;
    Ok(Json(serde_json::json!({"ok":true})))
}

fn account(identity: &Identity) -> Result<String, RegistryError> {
    identity
        .user_id()
        .map(str::to_owned)
        .ok_or_else(|| RegistryError::Unauthenticated { resource: "package stars".to_owned() })
}

pub(super) async fn own_stars(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
) -> Response {
    let result = match account(&identity) {
        Ok(id) => list(&state, &identity, registry.as_deref(), id).await,
        Err(error) => Err(error),
    };
    private_no_cache(result.map(Json).into_response())
}

pub(super) async fn user_stars(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<UserPath>,
) -> Response {
    let result = match state.inner.identity.auth.users.active_account_id(&path.user).await {
        Ok(Some(id)) => list(&state, &identity, registry.as_deref(), id).await,
        Ok(None) => Err(RegistryError::NotFound),
        Err(error) => Err(error),
    };
    private_no_cache(result.map(Json).into_response())
}

async fn list(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    id: String,
) -> Result<Vec<String>, RegistryError> {
    let registry =
        addressed_registry(state, registry, Ecosystem::Npm).ok_or(RegistryError::NotFound)?;
    if !registry_visible_to_caller(state, identity, &registry) {
        return Err(RegistryError::NotFound);
    }
    let mut visible = Vec::new();
    for (namespace, name) in state.inner.storage.sql_database().account_stars(id).await? {
        let RegistrySource::Hosted(source) = resolve_registry_source(state, &registry, &name)
        else {
            continue;
        };
        if matches!(hosted_gate(state, identity, &source, &name), HostedGate::Allowed(found) if found == namespace)
        {
            visible.push(name);
        }
    }
    Ok(visible)
}
