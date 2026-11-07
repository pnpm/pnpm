//! Removing one version a tarball `DELETE` names from its packument, so its
//! files' mappings go in that commit and their archives are reclaimed.

use super::{reclaim_unpublished_blobs, unpublished_blobs};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentWrite};
use serde_json::{Map, Value};

/// Remove `version` from `name`'s packument when it still lists it, and
/// reclaim the archives only it mapped. Reports whether it was listed. A
/// `dist-tags` entry naming it moves to the highest version left, or goes
/// when none is. A commit another writer lands first is read again and the
/// removal retried, so an unrelated change to the package does not leave
/// the version listed after the caller recorded it unpublished.
pub(super) async fn remove_listed_version(
    storage: &pnpr_storage::Storage,
    identity: &Identity,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<bool, RegistryError> {
    for _ in 0..DOCUMENT_WRITE_RETRIES {
        if let Some(listed) = try_remove_listed_version(storage, identity, name, version).await? {
            return Ok(listed);
        }
    }
    Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
}

/// One attempt at [`remove_listed_version`]; `None` when another writer
/// changed the packument since it was read.
async fn try_remove_listed_version(
    storage: &pnpr_storage::Storage,
    identity: &Identity,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<Option<bool>, RegistryError> {
    let Some(stored) = storage.read_hosted_document_for_update(name).await? else {
        return Ok(Some(false));
    };
    let mut packument: Value = serde_json::from_slice(&stored.bytes)?;
    let Some(object) = packument.as_object_mut() else { return Ok(Some(false)) };
    if !drop_version(object, version) {
        return Ok(Some(false));
    }
    let removed = [version.to_string()];
    let unpublished = unpublished_blobs(storage, name, &removed).await;
    let bytes = serde_json::to_vec(&packument)?;
    let principal = identity.user_id();
    match storage.write_hosted_document_if_current(name, &bytes, Some(&stored.version), principal)
        .await?
    {
        DocumentWrite::Written => {
            reclaim_unpublished_blobs(storage, name, unpublished).await;
            Ok(Some(true))
        }
        DocumentWrite::Conflict => Ok(None),
    }
}

/// Drop `version` from `packument`, with its `time` entry, and move the
/// tags that named it. Reports whether the packument listed it.
fn drop_version(packument: &mut Map<String, Value>, version: &str) -> bool {
    let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) else {
        return false;
    };
    if versions.shift_remove(version).is_none() {
        return false;
    }
    let highest = versions
        .keys()
        .filter_map(|left| node_semver::Version::parse(left).ok())
        .max()
        .map(|highest| highest.to_string());
    if let Some(time) = packument.get_mut("time").and_then(Value::as_object_mut) {
        time.shift_remove(version);
    }
    if let Some(tags) = packument.get_mut("dist-tags").and_then(Value::as_object_mut) {
        move_tags(tags, version, highest.as_deref());
    }
    true
}

/// Point every tag naming `version` at `highest`, or drop it without one.
fn move_tags(tags: &mut Map<String, Value>, version: &str, highest: Option<&str>) {
    let Some(highest) = highest else {
        tags.retain(|_, target| target.as_str() != Some(version));
        return;
    };
    for target in tags.values_mut() {
        if target.as_str() == Some(version) {
            *target = Value::String(highest.to_string());
        }
    }
}
