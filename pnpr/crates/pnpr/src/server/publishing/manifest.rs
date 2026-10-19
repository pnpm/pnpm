use std::collections::HashSet;

use serde_json::{Value, json};

use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::{
    HostedDocumentForUpdate, HostedDocumentVersion,
    publish::{extract_predicate_type, merge_manifest},
};

use super::attachments::{AttachmentKind, PreparedAttachment};

pub(super) fn record_publisher(incoming: &mut Value, identity: &Identity) {
    if let Identity::User { username, .. } = identity {
        record_publisher_in_maintainers(incoming, username);
    }
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for manifest in versions.values_mut().filter_map(Value::as_object_mut) {
        match identity {
            Identity::User { username, .. } => {
                manifest.insert("_npmUser".to_string(), json!({ "name": username }));
            }
            _ => {
                manifest.remove("_npmUser");
            }
        }
    }
}

fn record_publisher_in_maintainers(incoming: &mut Value, username: &str) {
    let Some(incoming_obj) = incoming.as_object_mut() else { return };
    let maintainers = incoming_obj
        .entry("maintainers")
        .or_insert_with(|| json!([]));
    let publisher = json!({ "name": username });
    let Some(list) = maintainers.as_array_mut() else {
        *maintainers = json!([publisher]);
        return;
    };
    let already_present = list
        .iter()
        .any(|entry| entry.get("name").and_then(Value::as_str) == Some(username));
    if !already_present {
        list.insert(0, publisher);
    }
}

/// The stored packument, parsed, with the version its update must be based
/// on. `None` for a package the org has not published yet.
pub(super) fn parse_hosted_packument(
    name: &CanonicalPackageName,
    hosted: Option<HostedDocumentForUpdate>,
) -> Result<(Option<Value>, Option<HostedDocumentVersion>), RegistryError> {
    let Some(packument) = hosted else {
        return Ok((None, None));
    };
    let value: Value = serde_json::from_slice(&packument.bytes).map_err(RegistryError::Json)?;
    if let Some(format_version) = value.get("formatVersion").and_then(Value::as_u64)
        && format_version > u64::from(pnpr_storage::publish::PACKUMENT_FORMAT_VERSION)
    {
        return Err(RegistryError::DocumentFormatTooNew {
            document: name.as_str().to_string(),
            format_version: format_version as u32,
            supported_version: pnpr_storage::publish::PACKUMENT_FORMAT_VERSION,
        });
    }
    Ok((Some(value), Some(packument.version)))
}

/// Validate each incoming version against the locally hosted packument. A
/// hosted packument is served as-is, so anything not in it is genuinely new.
/// Published content is immutable, so re-publishes with modified tarballs or
/// integrity are rejected. New versions must provide a tarball.
pub(super) fn check_publishable_versions(
    name: &CanonicalPackageName,
    incoming: &Value,
    hosted: Option<&Value>,
    prepared: &[PreparedAttachment],
) -> Result<(), RegistryError> {
    let attachment_versions: HashSet<&str> = prepared
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Tarball)
        .map(|attachment| attachment.version.as_str())
        .collect();
    let hosted_versions = hosted
        .and_then(|h| h.get("versions"))
        .and_then(Value::as_object);
    let Some(incoming_versions) = incoming.get("versions").and_then(Value::as_object) else {
        return Ok(());
    };
    for (version, incoming_manifest) in incoming_versions {
        let has_attachment = attachment_versions.contains(version.as_str());
        let Some(hosted_manifest) = hosted_versions.and_then(|hosted| hosted.get(version)) else {
            if has_attachment {
                continue;
            }
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "cannot publish version {version} of {:?} without a tarball",
                    name.as_str(),
                ),
            });
        };
        if has_attachment || integrity_changed(incoming_manifest, hosted_manifest) {
            return Err(RegistryError::VersionAlreadyPublished {
                package: name.as_str().to_string(),
                version: version.clone(),
            });
        }
    }
    Ok(())
}

/// Whether the incoming manifest declares an integrity other than the one
/// the hosted manifest carries.
pub(super) fn integrity_changed(incoming_manifest: &Value, hosted_manifest: &Value) -> bool {
    let incoming_integrity = incoming_manifest.pointer("/dist/integrity").and_then(Value::as_str);
    let hosted_integrity = hosted_manifest.pointer("/dist/integrity").and_then(Value::as_str);
    incoming_integrity.is_some_and(|integrity| Some(integrity) != hosted_integrity)
}

pub(super) fn prepare_staged_document(
    name: &CanonicalPackageName,
    hosted: Option<&Value>,
    incoming: &Value,
    prepared: &[PreparedAttachment],
    now_iso: &str,
) -> Result<Vec<u8>, RegistryError> {
    let mut merged = merge_manifest(hosted, incoming, hosted, now_iso);
    attach_sigstore_attestations(&mut merged, name, prepared);
    serde_json::to_vec_pretty(&merged).map_err(RegistryError::Json)
}

fn attach_sigstore_attestations(
    merged: &mut Value,
    name: &CanonicalPackageName,
    prepared: &[PreparedAttachment],
) {
    for attachment in prepared
        .iter()
        .filter(|att| att.kind == AttachmentKind::Sigstore)
    {
        if let Some(dist) = merged
            .get_mut("versions")
            .and_then(|v| v.get_mut(&attachment.version))
            .and_then(|m| m.get_mut("dist"))
            .and_then(Value::as_object_mut)
        {
            let predicate_type = extract_predicate_type(&attachment.attachment.data);
            dist.insert(
                "attestations".to_string(),
                json!({
                    "url": format!("/-/npm/v1/attestations/{}@{}", name.as_str(), attachment.version),
                    "provenance": { "predicateType": predicate_type },
                }),
            );
        }
    }
}
