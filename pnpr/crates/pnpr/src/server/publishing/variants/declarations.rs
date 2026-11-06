//! The variant archives a publish body attaches.

use super::{
    Declarations, GENERATED_ARCHIVE_FIELD, PreparedAttachment, RegistryError, STORED_FIELD, Value,
    VariantDeclaration,
};

/// Remove the server-owned variant fields from every version of a publish
/// body: pnpr derives them from the archives.
pub(in super::super) fn remove_server_fields(incoming: &mut Value) {
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for manifest in versions.values_mut().filter_map(Value::as_object_mut) {
        manifest.remove(STORED_FIELD);
        manifest.remove(GENERATED_ARCHIVE_FIELD);
    }
}

/// Put back the server-owned variant fields of every version of a rewritten
/// `incoming` packument from the `hosted` version it retains: a rewrite
/// carries what clients see, which lacks them, and may not set them.
pub(in super::super::super) fn restore_server_fields(hosted: &Value, incoming: &mut Value) {
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for (version, manifest) in versions.iter_mut() {
        let Some(manifest) = manifest.as_object_mut() else { continue };
        for field in [STORED_FIELD, GENERATED_ARCHIVE_FIELD] {
            match hosted.pointer(&format!("/versions/{}/{field}", escape_pointer(version))) {
                Some(kept) => manifest.insert(field.to_string(), kept.clone()),
                None => manifest.remove(field),
            };
        }
    }
}

/// A JSON pointer segment for `segment`.
fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// The variants each version attaches, by the tags that name their
/// attachments. What each archive is a variant for is read from the archive
/// itself, and must give the tag that names it.
pub(in super::super) fn attached_variants(prepared: &[PreparedAttachment]) -> Declarations {
    let mut declared = Declarations::new();
    for attachment in prepared {
        if let Some(tag) = &attachment.variant_tag {
            declared
                .entry(attachment.version.clone())
                .or_default()
                .push(VariantDeclaration { tag: tag.clone() });
        }
    }
    declared
}

/// Reject a variant attachment of a version the publish body does not
/// describe.
pub(in super::super) fn check_version(
    incoming: &Value,
    filename: &str,
    version: &str,
) -> Result<(), RegistryError> {
    if incoming
        .pointer("/versions")
        .and_then(|versions| versions.get(version))
        .is_some()
    {
        return Ok(());
    }
    Err(RegistryError::BadRequest {
        reason: format!(
            "the variant attachment {filename} names version {version}, which the \
                         publish does not describe",
        ),
    })
}
