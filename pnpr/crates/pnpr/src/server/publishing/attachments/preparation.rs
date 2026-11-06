//! Resolving each attachment of a publish to the archive it stores and the
//! integrity its bytes are verified against.

use super::{
    AttachmentKind, CanonicalPackageName, HashSet, PendingAttachment, PreparedAttachment,
    RegistryError, Value, variants,
};

pub(super) fn prepare_attachments(
    name: &CanonicalPackageName,
    incoming: &Value,
    attachments: Vec<PendingAttachment>,
) -> Result<Vec<PreparedAttachment>, RegistryError> {
    let mut prepared = Vec::with_capacity(attachments.len());
    let mut seen_canonical = HashSet::with_capacity(attachments.len());
    let mut tarball_versions = HashSet::new();

    for attachment in attachments {
        let item = prepare_attachment(name, incoming, attachment)?;
        if !seen_canonical.insert(item.canonical.clone()) {
            return Err(RegistryError::BadRequest {
                reason: format!("duplicate attachment for {}", item.canonical),
            });
        }
        // A version published without an `any` archive has only variants
        // until pnpr generates its archive, and a statement names them.
        if matches!(item.kind, AttachmentKind::Tarball | AttachmentKind::Variant) {
            tarball_versions.insert(item.version.clone());
        }
        prepared.push(item);
    }
    for att in &prepared {
        if att.kind == AttachmentKind::Sigstore && !tarball_versions.contains(&att.version) {
            return Err(RegistryError::BadRequest {
                reason: format!("sigstore attachment for {} has no tarball", att.version),
            });
        }
    }
    Ok(prepared)
}

/// Prepare an attachment to be verified against its version's `dist`, or a
/// variant's against the integrity it declares.
fn prepare_attachment(
    name: &CanonicalPackageName,
    incoming: &Value,
    attachment: PendingAttachment,
) -> Result<PreparedAttachment, RegistryError> {
    if let Some(parsed) = variants::parse_variant_attachment(name, &attachment.filename) {
        let parsed = parsed?;
        variants::check_version(incoming, &attachment.filename, &parsed.1)?;
        return variants::prepare_variant_attachment(attachment, parsed);
    }
    let (canonical, version, kind) = parse_attachment_kind(name, &attachment.filename)?;
    if kind == AttachmentKind::Sigstore && attachment.declared_integrity.is_some() {
        return Err(RegistryError::InvalidAttachment {
            filename: attachment.filename,
            reason: "a sigstore bundle is carried inline in `data`, with no `integrity`".to_string(
            ),
        });
    }
    let dist = incoming
        .get("versions")
        .and_then(|versions| versions.get(&version))
        .and_then(|manifest| manifest.get("dist"))
        .cloned()
        .unwrap_or(Value::Null);
    check_declared_integrity(&attachment, &dist)?;
    Ok(PreparedAttachment { attachment, canonical, version, dist, kind, variant_tag: None })
}

/// Refuse a version's archive that declares another integrity than its
/// version's `dist.integrity`, which is what its bytes are verified against.
fn check_declared_integrity(
    attachment: &PendingAttachment,
    dist: &Value,
) -> Result<(), RegistryError> {
    let Some(declared) = attachment.declared_integrity.as_deref() else { return Ok(()) };
    let expected = dist.get("integrity").and_then(Value::as_str);
    if expected == Some(declared) {
        return Ok(());
    }
    Err(RegistryError::InvalidAttachment {
        filename: attachment.filename.clone(),
        reason: format!(
            "EINTEGRITY: it declares the integrity {declared}, but its version's dist.integrity \
             is {}",
            expected.unwrap_or("absent"),
        ),
    })
}

fn parse_attachment_kind(
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<(String, String, AttachmentKind), RegistryError> {
    if filename.ends_with(".tgz") {
        let (canonical, version) = name.parse_tarball_name(filename)?;
        Ok((canonical, version, AttachmentKind::Tarball))
    } else if filename.ends_with(".sigstore") || filename.ends_with(".sigstore.json") {
        let (canonical, version) = name.parse_sigstore_name(filename)?;
        Ok((canonical, version, AttachmentKind::Sigstore))
    } else {
        Err(RegistryError::InvalidTarballName {
            package: name.as_str().to_string(),
            filename: filename.to_string(),
        })
    }
}
