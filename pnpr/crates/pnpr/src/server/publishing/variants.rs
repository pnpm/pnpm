//! Platform variants of hosted versions
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md)).
//!
//! A version publishes its `any` archive's `.tgz` plus one `.tgz` per variant,
//! attached as `<basename>-<version>~<tag>.tgz` with the `integrity` the
//! archive is verified against. Each archive declares itself: its
//! `package.json` has a `variant` field listing the platform tags it runs
//! on, from which pnpr derives the variant's tag and rejects the version when
//! it is not the tag that names the archive. The registry checks the variants against the version,
//! records what it derived under the server-owned `_pnprVariants`, and
//! describes it as `variants` in its own package document only.
//!
//! Each archive travels in the publish body, or is staged by integrity
//! before it and referenced from its attachment (see
//! [`crate::server::staged_archives`]), so a version's archives together are
//! not bounded by one request body.

pub(in super::super) use declarations::restore_server_fields;
pub(super) use declarations::{attached_variants, check_version, remove_server_fields};
pub(super) use verification::attach_variants;

mod declarations;
mod verification;

use super::{
    Integrity, PendingAttachment,
    attachments::{AttachmentKind, PreparedAttachment, PublishedTar},
};
use pnpm_platform_variants::{MAX_VARIANT_TAG_BYTES, is_valid_variant_tag};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The server-owned field the stored packument keeps verified variants in.
pub(in super::super) const STORED_FIELD: &str = "_pnprVariants";
/// The server-owned field of a version published without an `any` archive,
/// holding the manifest its variants share, which describes the version in
/// pnpr's document. Its `dist` describes the archive pnpr generated for npm's
/// documents.
pub(in super::super) const GENERATED_ARCHIVE_FIELD: &str = "_pnprGeneratedArchive";
/// The field of `dist.pnpr` that [`describe_version_variants`] moves
/// [`GENERATED_ARCHIVE_FIELD`] into.
pub(in super::super) const SHARED_MANIFEST_FIELD: &str = "sharedManifest";

/// One variant archive a version attaches, by the tag that names it.
#[derive(Debug, Clone)]
pub(in super::super) struct VariantDeclaration {
    pub(in super::super) tag: String,
}

/// Attached variants, by version.
pub(in super::super) type Declarations = BTreeMap<String, Vec<VariantDeclaration>>;

/// What publishing a variant's `.tgz` derived from it.
pub(in super::super) struct VariantTar {
    /// The tag that names the archive's attachment.
    pub(in super::super) tag: String,
    /// The `.tgz` integrity as published.
    pub(in super::super) tgz_integrity: Option<String>,
    pub(in super::super) tar_integrity: String,
    pub(in super::super) manifest: Option<Value>,
    /// The digests of the variant's `.tgz` and its `package.json`, which its
    /// entry of the version's release record names as its inputs.
    pub(in super::super) inputs: Value,
}

/// What a published variant contributes to the stored version.
pub(in super::super) struct PublishedVariant {
    pub(in super::super) version: String,
    pub(in super::super) tar: VariantTar,
    pub(in super::super) inspection: pnpm_tarball::TarInspection,
}

fn bad_request(reason: String) -> RegistryError {
    RegistryError::BadRequest { reason }
}

/// Why `tag` cannot name a variant attachment, or `None` when it can. Whether
/// it is the tag the archive's platforms give is checked once the archive is
/// read.
pub(in super::super) fn invalid_tag(tag: &str) -> Option<String> {
    if tag.len() > MAX_VARIANT_TAG_BYTES {
        return Some(format!(
            "the variant tag {tag:?} is {} bytes, over the {MAX_VARIANT_TAG_BYTES}-byte limit; \
             split the variant into variants of fewer platforms",
            tag.len(),
        ));
    }
    (!is_valid_variant_tag(tag)).then(|| format!("{tag:?} is not a variant tag"))
}

/// Parse a variant attachment filename, `<basename>-<version>~<tag>.tgz`,
/// into its canonical blob name, version, and tag. `None` when `filename`
/// is not a variant attachment.
pub(in super::super) fn parse_variant_attachment(
    name: &CanonicalPackageName,
    filename: &str,
) -> Option<Result<(String, String, String), RegistryError>> {
    let stem = filename.strip_suffix(".tgz")?;
    let (archive_stem, tag) = stem.rsplit_once('~')?;
    // A package name may hold `~` itself: unless what precedes the last `~`
    // is this package's tarball, the attachment is the package's own.
    let (_, version) = name
        .parse_tarball_name(&format!("{archive_stem}.tgz"))
        .ok()?;
    if let Some(reason) = invalid_tag(tag) {
        return Some(Err(bad_request(format!("{filename:?}: {reason}"))));
    }
    let blob = variant_blob_name(name, &version, tag);
    Some(Ok((blob, version, tag.to_string())))
}

/// The blob a hosted variant's `.tgz` is stored in.
pub(in super::super) fn variant_blob_name(
    name: &CanonicalPackageName,
    version: &str,
    tag: &str,
) -> String {
    let any_name = name.tarball_name_for_version(version);
    let stem = any_name.strip_suffix(".tgz").unwrap_or(&any_name);
    format!("{stem}~{tag}.tgz")
}

/// The stored variant of a version whose tag is `tag`, with its unpacked
/// size when known, or `None` when the version has no such variant.
pub(in super::super) fn stored_variant_size(manifest: &Value, tag: &str) -> Option<Option<u64>> {
    manifest
        .get(STORED_FIELD)?
        .as_array()?
        .iter()
        .find(|variant| variant.get("tag").and_then(Value::as_str) == Some(tag))
        .map(|variant| variant.get("unpackedSize").and_then(Value::as_u64))
}

/// Prepare a variant attachment, parsed by [`parse_variant_attachment`], to
/// be verified against the `integrity` it declares.
pub(super) fn prepare_variant_attachment(
    attachment: PendingAttachment,
    (canonical, version, tag): (String, String, String),
) -> Result<PreparedAttachment, RegistryError> {
    let Some(integrity) = attachment.declared_integrity.clone() else {
        return Err(bad_request(format!(
            "the variant attachment {} has no integrity",
            attachment.filename,
        )));
    };
    Ok(PreparedAttachment {
        attachment,
        canonical,
        version,
        dist: json!({ "integrity": integrity }),
        kind: AttachmentKind::Variant,
        variant_tag: Some(tag),
    })
}

/// What a variant's `.tgz` derived, with the root `package.json` an
/// installer selects from its archive, its install-critical fields
/// normalized as the version's are, so the two compare field for field.
pub(super) fn variant_tar(
    tag: String,
    integrity: &Integrity,
    tgz_integrity: Option<&str>,
    archive: &pnpm_tarball::PublicationArchive,
) -> VariantTar {
    let manifest = pnpm_package_manifest::parse_manifest_bytes(&archive.manifest)
        .ok()
        .and_then(|manifest| match manifest {
            Value::Object(mut fields) => {
                let facts = pnpm_package_manifest::ArchiveFacts {
                    binding_gyp: archive.binding_gyp,
                    has_shrinkwrap: archive.shrinkwrap_integrity.is_some(),
                    files: &archive.files,
                };
                let derived = pnpm_package_manifest::InstallManifest::derive(
                    &Value::Object(fields.clone()),
                    facts,
                );
                derived.write_to_version(&mut fields);
                Some(Value::Object(fields))
            }
            _ => None,
        });
    let inputs = super::release_records::archive_inputs(tgz_integrity, archive);
    VariantTar {
        tag,
        tgz_integrity: tgz_integrity.map(str::to_string),
        tar_integrity: integrity.to_string(),
        manifest,
        inputs,
    }
}

/// Split a variant's tar from the tars of `any` archive's `.tgz` files.
pub(super) fn split_variant(tar: PublishedTar) -> Result<PublishedVariant, PublishedTar> {
    match tar {
        PublishedTar {
            version,
            inspection,
            variant: Some(tar),
            ..
        } => Ok(PublishedVariant { version, tar, inspection }),
        tar => Err(tar),
    }
}

/// Move each version's stored variants into `dist.pnpr.variants`, where
/// pnpr's document reads them. The client derives each variant's route from
/// its tag, so no URL is written.
pub fn describe_variants(doc: &mut Value) {
    let Some(versions) = doc.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for manifest in versions.values_mut() {
        describe_version_variants(manifest);
    }
}

/// [`describe_variants`] for one version manifest.
pub fn describe_version_variants(manifest: &mut Value) {
    let Some(manifest) = manifest.as_object_mut() else { return };
    let Some(Value::Array(variants)) = manifest.remove(STORED_FIELD) else { return };
    let shared = manifest.remove(GENERATED_ARCHIVE_FIELD);
    let Some(dist) = manifest.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    let pnpr = dist
        .entry("pnpr")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(pnpr) = pnpr.as_object_mut() {
        pnpr.insert("variants".into(), Value::Array(variants));
        if let Some(shared) = shared {
            pnpr.insert(SHARED_MANIFEST_FIELD.into(), shared);
        }
    }
}
