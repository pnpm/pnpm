use serde_json::Value;

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::Storage;

use crate::server::AppState;

pub(in super::super) async fn check_publish_tombstone(
    state: &AppState,
    identity: &Identity,
    storage: &Storage,
    name: &CanonicalPackageName,
) -> Result<(), RegistryError> {
    let Some(tombstone) = storage.read_tombstone(name).await? else {
        return Ok(());
    };
    if storage.read_hosted_document(name).await?.is_some() {
        return Ok(());
    }
    let username = identity.username();
    let is_admin =
        username.is_some_and(|user| state.inner.config.identity.admins.contains(user));
    let is_former_maintainer = username.is_some_and(|user| {
        tombstone.maintainers
            .iter()
            .any(|maintainer| maintainer == user)
    });
    if !is_admin && !is_former_maintainer {
        return Err(RegistryError::PackageTombstoned { package: name.as_str().to_string() });
    }
    Ok(())
}

pub(super) async fn check_tombstoned_versions(
    storage: &Storage,
    name: &CanonicalPackageName,
    incoming: &Value,
) -> Result<(), RegistryError> {
    let Some(tombstone) = storage.read_tombstone(name).await? else {
        return Ok(());
    };
    let Some(incoming_versions) = incoming.get("versions").and_then(Value::as_object) else {
        return Ok(());
    };
    for version in incoming_versions.keys() {
        if tombstone.unpublished_versions.contains(version) {
            return Err(RegistryError::CannotRepublishUnpublishedVersion {
                package: name.as_str().to_string(),
                version: version.clone(),
            });
        }
    }
    Ok(())
}
