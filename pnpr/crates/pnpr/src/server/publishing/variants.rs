//! Platform variants (proof of concept for the pnpr platform-variants RFC).
//!
//! A version publishes its primary `.tgz` plus one `.tgz` per variant,
//! attached as `<basename>-<version>~<label>.tgz`. The publish body
//! references each variant archive under `versions[<version>].pnprVariants`
//! as `{ "<label>": "<.tgz integrity>" }`, the integrity the upload is
//! verified against; the label only pairs the reference with its attachment.
//! Each archive declares itself: its `package.json` has a `variant` field
//! listing the platform tags it runs on, from which pnpr derives the
//! variant's tag. The registry checks the variants against the version,
//! records what it derived under the server-owned `_pnprVariants`, and
//! describes it as `variants` in its own package document only.

pub(super) use declarations::take_declarations;

mod declarations;
mod verification;

use super::{
    Integrity, PendingAttachment,
    attachments::{AttachmentKind, PreparedAttachment, PublishedTar, WrittenAttachments},
    cleanup_tmp_slots,
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, io::Read};

/// The field a publish body declares variants in.
const DECLARATION_FIELD: &str = "pnprVariants";
/// The server-owned field the stored packument keeps verified variants in.
pub(in super::super) const STORED_FIELD: &str = "_pnprVariants";
/// The field of a stored variant naming the attachment its blob is stored
/// under. pnpr's document leaves it out.
const LABEL_FIELD: &str = "label";

/// One variant archive a version references.
#[derive(Debug, Clone)]
pub(in super::super) struct VariantDeclaration {
    pub(in super::super) label: String,
    pub(in super::super) tgz_integrity: String,
}

/// Declared variants, by version.
pub(in super::super) type Declarations = BTreeMap<String, Vec<VariantDeclaration>>;

/// What publishing a variant's `.tgz` derived from it.
pub(in super::super) struct VariantTar {
    pub(in super::super) label: String,
    pub(in super::super) tar_integrity: String,
    pub(in super::super) manifest: Option<Value>,
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

/// Whether `label` is a valid attachment label: lowercase letters, digits,
/// `.`, `_`, and `-`.
pub(in super::super) fn is_valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 64
        && !label.starts_with('.')
        && label
            .bytes()
            .all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
}

/// Parse a variant attachment filename, `<basename>-<version>~<label>.tgz`,
/// into its canonical blob name, version, and label. `None` when `filename`
/// is not a variant attachment.
pub(in super::super) fn parse_variant_attachment(
    name: &CanonicalPackageName,
    filename: &str,
) -> Option<Result<(String, String, String), RegistryError>> {
    let stem = filename.strip_suffix(".tgz")?;
    let (primary_stem, label) = stem.rsplit_once('~')?;
    if !is_valid_label(label) {
        return Some(Err(bad_request(format!("invalid variant label in {filename:?}"))));
    }
    Some(
        name.parse_tarball_name(&format!("{primary_stem}.tgz"))
            .map(|(_, version)| {
                (variant_blob_name(name, &version, label), version, label.to_string())
            }),
    )
}

/// The blob a hosted variant's `.tgz` is stored in.
pub(in super::super) fn variant_blob_name(
    name: &CanonicalPackageName,
    version: &str,
    label: &str,
) -> String {
    let primary = name.tarball_name_for_version(version);
    let stem = primary.strip_suffix(".tgz").unwrap_or(&primary);
    format!("{stem}~{label}.tgz")
}

/// The label of the stored variant of a version whose tag is `tag`, and
/// its unpacked size.
pub(in super::super) fn stored_variant_label(
    manifest: &Value,
    tag: &str,
) -> Option<(String, Option<u64>)> {
    manifest
        .get(STORED_FIELD)?
        .as_array()?
        .iter()
        .find(|variant| variant.get("tag").and_then(Value::as_str) == Some(tag))
        .and_then(|variant| {
            let label = variant
                .get(LABEL_FIELD)?
                .as_str()?
                .to_string();
            Some((label, variant.get("unpackedSize").and_then(Value::as_u64)))
        })
}

/// Prepare a variant attachment, parsed by [`parse_variant_attachment`], to
/// be verified against its declared `.tgz` integrity.
pub(super) fn prepare_variant_attachment(
    attachment: PendingAttachment,
    (canonical, version, label): (String, String, String),
    declared: &Declarations,
) -> Result<PreparedAttachment, RegistryError> {
    let dist = attachment_dist(declared, &version, &label)?;
    Ok(PreparedAttachment {
        attachment,
        canonical,
        version,
        dist,
        kind: AttachmentKind::Variant,
        variant_label: Some(label),
    })
}

/// The `dist` a variant attachment is verified against.
fn attachment_dist(
    declared: &Declarations,
    version: &str,
    label: &str,
) -> Result<Value, RegistryError> {
    declared
        .get(version)
        .and_then(|variants| {
            variants
                .iter()
                .find(|variant| variant.label == label)
        })
        .map(|variant| json!({ "integrity": variant.tgz_integrity }))
        .ok_or_else(|| {
            bad_request(format!("attachment for variant {label:?} of {version} is not declared"))
        })
}

/// Keep the slot of a `.tgz` that does not decode to a tar. A variant must
/// decode, so its slot is removed and the publish fails.
pub(super) async fn keep_undecoded(
    slot: pnpr_storage::BlobSlot,
    kind: AttachmentKind,
    canonical: &str,
) -> Result<pnpr_storage::BlobSlot, RegistryError> {
    if kind != AttachmentKind::Variant {
        return Ok(slot);
    }
    let _ = tokio::fs::remove_file(&slot.tmp_path).await;
    Err(bad_request(format!("{canonical} does not decode to a tar")))
}

/// What a variant's `.tgz`, written to `path`, derived; `None` for any other
/// attachment.
pub(super) fn variant_tar(
    label: Option<String>,
    integrity: &Integrity,
    path: &std::path::Path,
) -> Option<VariantTar> {
    label.map(|label| VariantTar {
        label,
        tar_integrity: integrity.to_string(),
        manifest: read_tgz_manifest(path),
    })
}

/// Split a variant's tar from the tars of primary `.tgz` files.
pub(super) fn split_variant(tar: PublishedTar) -> Result<PublishedVariant, PublishedTar> {
    match tar {
        PublishedTar {
            version,
            inspection,
            variant: Some(tar),
        } => Ok(PublishedVariant { version, tar, inspection }),
        tar => Err(tar),
    }
}

/// Read `package.json` from a `.tgz`.
fn read_tgz_manifest(path: &std::path::Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = flate2::read::MultiGzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().ok()? {
        let mut entry = entry.ok()?;
        let path = entry.path().ok()?.into_owned();
        let mut components = path.components();
        components.next();
        if components.as_path() == std::path::Path::new("package.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).ok()?;
            return serde_json::from_slice(&bytes).ok();
        }
    }
    None
}

/// Record the written variants on the merged document (see
/// [`verification::attach_variants`]). A failure removes every written slot.
pub(super) async fn attach_written_variants(
    merged: &mut Value,
    declared: &Declarations,
    mut written: WrittenAttachments,
) -> Result<WrittenAttachments, RegistryError> {
    let published = std::mem::take(&mut written.variants);
    if let Err(err) = verification::attach_variants(merged, declared, &published) {
        cleanup_tmp_slots(written.slots).await;
        return Err(err);
    }
    Ok(written)
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
    let Some(Value::Array(mut variants)) = manifest.remove(STORED_FIELD) else { return };
    for variant in &mut variants {
        if let Some(variant) = variant.as_object_mut() {
            variant.remove(LABEL_FIELD);
        }
    }
    let Some(dist) = manifest.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    let pnpr = dist
        .entry("pnpr")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(pnpr) = pnpr.as_object_mut() {
        pnpr.insert("variants".into(), Value::Array(variants));
    }
}
