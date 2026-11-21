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
    resolve_write_target,
};
use crate::server::package_mutation::maintainers::{
    OwnerEdit,
    update_package_maintainers,
};
use axum::{
    Json,
    response::IntoResponse,
};
use pnpr_policy::Identity;

#[derive(Deserialize)]
pub(super) struct Owner {
    user: String,
}

#[derive(Deserialize)]
pub(super) struct OwnerPath {
    name: String,
    user: String,
}

pub(super) async fn add_owner(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<super::npm_handlers::NamePath>,
    Json(owner): Json<Owner>,
) -> Response {
    edit_owner(&state, &identity, registry.as_deref(), (&path.name, OwnerEdit::Add(&owner.user)))
        .await
}

pub(super) async fn remove_owner(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<OwnerPath>,
) -> Response {
    edit_owner(&state, &identity, registry.as_deref(), (&path.name, OwnerEdit::Remove(&path.user)))
        .await
}

async fn edit_owner(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    (name, edit): (&str, OwnerEdit<'_>),
) -> Response {
    let name = match CanonicalPackageName::parse(name, Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    update_package_maintainers(state, identity, &target, (&name, edit)).await
}
