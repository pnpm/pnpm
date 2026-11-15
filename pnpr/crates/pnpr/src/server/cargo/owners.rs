//! `cargo owner`: the crates.io owners API over a hosted crate's owners.

use super::{
    super::package_owners::{OwnerChange, OwnerRequest, change_owners, list_owners},
    AppState, AuthedCaller, CrateDocument, HashMap, Path, RegistryError, Response, State,
    StatusCode, TargetRegistry, error_response, json_response, private_no_cache,
};
use axum::body::Bytes;

/// `GET api/v1/crates/<crate>/owners` — `cargo owner --list`.
pub(super) async fn get_owners(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let listed =
        list_owners::<CrateDocument>(&state, &identity, registry.as_deref(), params.get("name"))
            .await;
    owners_response(listed)
}

/// `PUT api/v1/crates/<crate>/owners` — `cargo owner --add`.
pub(super) async fn put_owners(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let request = OwnerRequest { registry: registry.as_deref(), name: params.get("name"), body };
    owners_response(
        change_owners::<CrateDocument>(&state, &identity, request, OwnerChange::Add).await,
    )
}

/// `DELETE api/v1/crates/<crate>/owners` — `cargo owner --remove`.
pub(super) async fn delete_owners(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let request = OwnerRequest { registry: registry.as_deref(), name: params.get("name"), body };
    owners_response(
        change_owners::<CrateDocument>(&state, &identity, request, OwnerChange::Remove).await,
    )
}

fn owners_response(result: Result<serde_json::Value, RegistryError>) -> Response {
    match result {
        Ok(body) => private_no_cache(json_response(StatusCode::OK, &body)),
        Err(err) => private_no_cache(error_response(err)),
    }
}
