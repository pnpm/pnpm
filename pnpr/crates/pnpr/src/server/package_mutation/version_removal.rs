//! Removing one version a tarball `DELETE` names from its packument, so its
//! files' mappings go in that commit and their archives are reclaimed.

use super::{
    super::{Action, AppState, WriteTarget, hosted_storage},
    read_authorized_packument, reclaim_unpublished_blobs, unpublished_blobs,
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentWrite, wait_after_document_write_conflict};
use serde_json::{Map, Value};

/// Remove `version` from `name`'s packument when it still lists it, and
/// reclaim the archives only it mapped. Reports whether it was listed. The
/// `latest` tag naming it moves to the highest version left, or goes when
/// none is. Other tags naming it are removed. Concurrent commits cause a retry.
/// The removed version and former maintainers are recorded together.
pub(super) async fn remove_listed_version(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    (name, version): (&CanonicalPackageName, &str),
) -> Result<bool, RegistryError> {
    for attempt in 0..DOCUMENT_WRITE_RETRIES {
        let outcome = try_remove_listed_version(state, identity, target, (name, version)).await?;
        match outcome {
            Some(listed) => return Ok(listed),
            None if attempt + 1 < DOCUMENT_WRITE_RETRIES => {
                wait_after_document_write_conflict(attempt).await;
            }
            None => {}
        }
    }
    Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
}

/// One attempt at [`remove_listed_version`]; `None` when another writer
/// changed the packument since it was read.
async fn try_remove_listed_version(
    state: &AppState,
    identity: &Identity,
    target: &WriteTarget,
    (name, version): (&CanonicalPackageName, &str),
) -> Result<Option<bool>, RegistryError> {
    let storage = hosted_storage(state, Some(&target.namespace));
    let Some((mut packument, doc_version)) =
        read_authorized_packument(state, identity, (target, name), Action::Unpublish).await?
    else {
        if storage.read_tombstone(name).await?.is_none() {
            return Err(RegistryError::NotFound);
        }
        return Ok(Some(false));
    };
    let Some(object) = packument.as_object_mut() else { return Ok(Some(false)) };
    if !drop_version(object, version) {
        return Ok(Some(false));
    }
    let removed = [version.to_string()];
    let unpublished = unpublished_blobs(&storage, name, &removed).await;
    let bytes = serde_json::to_vec(&packument)?;
    let principal = identity.user_id();
    match storage.write_hosted_document_if_current(name, &bytes, Some(&doc_version), principal)
        .await?
    {
        DocumentWrite::Written => {
            reclaim_unpublished_blobs(&storage, name, unpublished).await;
            Ok(Some(true))
        }
        DocumentWrite::Conflict => Ok(None),
    }
}

/// Drop `version` from `packument`, with its `time` entry, and update the
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

fn move_tags(tags: &mut Map<String, Value>, version: &str, highest: Option<&str>) {
    tags.retain(|tag, target| {
        if target.as_str() != Some(version) {
            return true;
        }
        if tag == "latest"
            && let Some(highest) = highest
        {
            *target = Value::String(highest.to_string());
            return true;
        }
        false
    });
}

#[cfg(test)]
#[path = "version_removal_tests.rs"]
mod tests;
