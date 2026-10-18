use serde_json::Value;

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{PackageTombstone, Storage};

use super::attachments::ValidatedPublish;
use crate::server::AppState;

/// Reject a publish that would recreate a deleted package, unless the caller
/// is an admin or one of its former maintainers. Runs before the package
/// lock, so [`check_staged_tombstone`] repeats it under the lock.
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
    check_may_reclaim(state, identity, &tombstone, name)
}

/// The tombstone checks a publish needs under the package lock: reclaiming
/// a deleted package, and republishing an unpublished version.
pub(super) async fn check_staged_tombstone(
    state: &AppState,
    storage: &Storage,
    doc: &ValidatedPublish,
    package_exists: bool,
) -> Result<(), RegistryError> {
    let Some(tombstone) = storage.read_tombstone(&doc.name).await? else {
        return Ok(());
    };
    if !package_exists {
        check_may_reclaim(state, &doc.publisher, &tombstone, &doc.name)?;
    }
    check_unpublished_versions(&tombstone, &doc.name, &doc.incoming)
}

fn check_may_reclaim(
    state: &AppState,
    identity: &Identity,
    tombstone: &PackageTombstone,
    name: &CanonicalPackageName,
) -> Result<(), RegistryError> {
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

fn check_unpublished_versions(
    tombstone: &PackageTombstone,
    name: &CanonicalPackageName,
    incoming: &Value,
) -> Result<(), RegistryError> {
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
