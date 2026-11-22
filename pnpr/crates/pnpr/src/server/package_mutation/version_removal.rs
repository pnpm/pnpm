//! Removing one version a tarball `DELETE` names from its packument, so its
//! files' mappings go in that commit and their archives are reclaimed.

use super::{
    super::{
        Action,
        AppState,
        WriteTarget,
        hosted_storage,
    },
    read_authorized_packument,
    reclaim_unpublished_blobs,
    unpublished_blobs,
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
    Map,
    Value,
};

/// Remove `version` from `name`'s packument when it still lists it, and
/// reclaim the archives only it mapped. Reports whether it was listed. The
/// `latest` tag returns to its most recent surviving target. Other tags
/// naming the removed version are dropped.
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
        read_authorized_packument(state, identity, (target, name), Action::Delete).await?
    else {
        if storage.read_tombstone(name).await?.is_none() {
            return Err(RegistryError::NotFound);
        }
        return Ok(Some(false));
    };
    let replacement = if packument.pointer("/dist-tags/latest").and_then(Value::as_str)
        == Some(version)
    {
        storage
            .previous_published_latest(name, version)
            .await?
            .or_else(|| highest_remaining_version(packument.get("versions")?.as_object()?, version))
    } else {
        None
    };
    let Some(object) = packument.as_object_mut() else { return Ok(Some(false)) };
    if !drop_version(object, version, replacement.as_deref()) {
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

fn highest_remaining_version(versions: &Map<String, Value>, removed: &str) -> Option<String> {
    versions
        .keys()
        .filter(|key| key.as_str() != removed)
        .filter_map(|key| {
            let parsed = node_semver::Version::parse(key).ok()?;
            Some((!parsed.is_prerelease(), parsed, key))
        })
        .max()
        .map(|(_, _, key)| key.clone())
}

/// Drop `version` from `packument`, with its `time` entry, and update the
/// tags that named it. Reports whether the packument listed it.
fn drop_version(
    packument: &mut Map<String, Value>,
    version: &str,
    replacement: Option<&str>,
) -> bool {
    let Some(versions) = packument.get_mut("versions").and_then(Value::as_object_mut) else {
        return false;
    };
    if versions.shift_remove(version).is_none() {
        return false;
    }
    if let Some(time) = packument.get_mut("time").and_then(Value::as_object_mut) {
        time.shift_remove(version);
    }
    if let Some(tags) = packument.get_mut("dist-tags").and_then(Value::as_object_mut) {
        move_tags(tags, version, replacement);
    }
    true
}

fn move_tags(tags: &mut Map<String, Value>, version: &str, replacement: Option<&str>) {
    tags.retain(|tag, target| {
        if target.as_str() != Some(version) {
            return true;
        }
        if tag == "latest"
            && let Some(replacement) = replacement
        {
            *target = Value::String(replacement.to_string());
            return true;
        }
        false
    });
}

#[cfg(test)]
#[path = "version_removal_tests.rs"]
mod tests;
