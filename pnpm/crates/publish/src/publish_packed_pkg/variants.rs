//! Publishing a version with its platform variants to pnpr
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#sending-a-version-with-variants)).
//!
//! Every archive is staged by its integrity before the document is sent, and
//! the document references each one: the `any` archive as the version's
//! `<name>-<version>.tgz`, each platform variant as
//! `<name>-<version>~<tag>.tgz`. A version without an `any` archive has no
//! `dist` and no plain attachment; pnpr generates the archive npm's
//! documents point to.

use super::{PackedPkg, PublishPackedPkgError, Value, clean_version, registry_for_display};
use crate::{registry_config_keys::NormalizedRegistryUrl, staged_archives::StagedArchive};
use std::collections::HashSet;

/// One platform variant's archive, published with its version.
#[derive(Debug, Clone)]
pub struct PackedVariant {
    /// The variant's tag, which names its attachment.
    pub tag: String,
    pub tarball_data: Vec<u8>,
}

/// Refuse to publish variants to a registry that is not declared with
/// `serverType: pnpr`: a version published without its variants cannot get
/// them later.
pub(super) fn check_registry(
    pkg: &PackedPkg<'_>,
    pnpr_registries: &HashSet<String>,
    registry: &NormalizedRegistryUrl,
) -> Result<(), PublishPackedPkgError> {
    if pkg.variants.is_empty() || pnpr_registries.contains(registry.as_str()) {
        return Ok(());
    }
    Err(PublishPackedPkgError::VariantsNeedPnpr { registry: registry_for_display(registry) })
}

/// Refuse to sign provenance for a version with variants, which pnpm cannot
/// do yet, before any archive is staged and in a dry run too.
pub(super) fn check_provenance(
    pkg: &PackedPkg<'_>,
    provenance: Option<bool>,
) -> Result<(), PublishPackedPkgError> {
    if !pkg.variants.is_empty() && provenance == Some(true) {
        return Err(PublishPackedPkgError::VariantsWithProvenance);
    }
    Ok(())
}

/// The archives to stage before the document that references them: the
/// `any` archive, whose integrity is `any_integrity`, and every variant.
pub(super) fn staged_archives<'a>(
    pkg: &PackedPkg<'a>,
    any_integrity: &str,
) -> Vec<StagedArchive<'a>> {
    let any = pkg.any_archive.then(|| StagedArchive {
        integrity: any_integrity.to_string(),
        bytes: pkg.tarball_data,
    });
    any.into_iter()
        .chain(
            pkg.variants
                .iter()
                .map(|variant| StagedArchive {
                    integrity: sri_sha512(&variant.tarball_data),
                    bytes: &variant.tarball_data,
                }),
        )
        .collect()
}

/// Make `document` reference the staged archives instead of carrying them:
/// the `any` archive's attachment names its integrity, each variant gets an
/// attachment named after it, and a version without an `any` archive loses
/// its `dist` and plain attachment. `version` is the manifest's, which may
/// carry build metadata.
pub(super) fn reference_staged_archives(document: &mut Value, pkg: &PackedPkg<'_>, version: &str) {
    let Some(attachments) = document.get_mut("_attachments").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(any_name) = attachments
        .keys()
        .find(|name| name.ends_with(".tgz"))
        .cloned()
    else {
        return;
    };
    let any_attachment = attachments.shift_remove(&any_name);
    if pkg.any_archive
        && let Some(mut attachment) = any_attachment
    {
        if let Some(attachment) = attachment.as_object_mut() {
            attachment.shift_remove("data");
            attachment.insert("integrity".into(), Value::String(sri_sha512(pkg.tarball_data)));
        }
        attachments.insert(any_name.clone(), attachment);
    }
    let stem = any_name.strip_suffix(".tgz").unwrap_or(&any_name);
    for variant in pkg.variants {
        attachments.insert(
            format!("{stem}~{}.tgz", variant.tag),
            serde_json::json!({
                "content_type": "application/octet-stream",
                "integrity": sri_sha512(&variant.tarball_data),
                "length": variant.tarball_data.len(),
            }),
        );
    }
    // The document keys the version as npm does, without build metadata.
    let key = clean_version(version).unwrap_or_else(|_| version.to_string());
    if !pkg.any_archive
        && let Some(version) = document
            .pointer_mut("/versions")
            .and_then(|versions| versions.get_mut(&key))
            .and_then(Value::as_object_mut)
    {
        version.shift_remove("dist");
    }
}

/// The SRI SHA-512 of `bytes`, the integrity a staged archive is named by.
pub(crate) fn sri_sha512(bytes: &[u8]) -> String {
    ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(bytes)
        .result()
        .to_string()
}

#[cfg(test)]
mod tests;
