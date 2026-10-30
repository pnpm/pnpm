//! Platform variants (proof of concept for the pnpr platform-variants RFC).
//!
//! A version publishes its primary `.tgz` plus one `.tgz` per variant,
//! attached as `<basename>-<version>~<tag>.tgz`. The publish body declares
//! each variant under `versions[<version>].pnprVariants` as
//! `{ "tag", "platforms": [{ "os", "cpu", "libc"? }], "integrity" }`, where
//! `integrity` is the `.tgz` digest the upload is verified against. The
//! registry checks the variants against the primary, records what it derived
//! under the server-owned `_pnprVariants`, and serves it as
//! `dist.pnpr.variants`.

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

/// One declared variant of one version.
#[derive(Debug, Clone)]
pub(in super::super) struct VariantDeclaration {
    pub(in super::super) tag: String,
    pub(in super::super) platforms: Vec<Value>,
    pub(in super::super) tgz_integrity: String,
}

/// Declared variants, by version.
pub(in super::super) type Declarations = BTreeMap<String, Vec<VariantDeclaration>>;

/// What publishing a variant's `.tgz` derived from it.
pub(in super::super) struct VariantTar {
    pub(in super::super) tag: String,
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

/// Whether `tag` is a valid variant tag: lowercase letters, digits, `.`,
/// `_`, and `-`.
pub(in super::super) fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 64
        && tag
            .bytes()
            .all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
}

/// Parse a variant attachment filename, `<basename>-<version>~<tag>.tgz`, into
/// its canonical blob name, version, and tag. `None` when `filename` is not a
/// variant attachment.
pub(in super::super) fn parse_variant_attachment(
    name: &CanonicalPackageName,
    filename: &str,
) -> Option<Result<(String, String, String), RegistryError>> {
    let stem = filename.strip_suffix(".tgz")?;
    let (primary_stem, tag) = stem.rsplit_once('~')?;
    if !is_valid_tag(tag) {
        return Some(Err(bad_request(format!("invalid variant tag in {filename:?}"))));
    }
    Some(
        name.parse_tarball_name(&format!("{primary_stem}.tgz"))
            .map(|(_, version)| (variant_blob_name(name, &version, tag), version, tag.to_string())),
    )
}

/// The blob a hosted variant's `.tgz` is stored in.
pub(in super::super) fn variant_blob_name(
    name: &CanonicalPackageName,
    version: &str,
    tag: &str,
) -> String {
    let primary = name.tarball_name_for_version(version);
    let stem = primary.strip_suffix(".tgz").unwrap_or(&primary);
    format!("{stem}~{tag}.tgz")
}

/// Prepare a variant attachment, parsed by [`parse_variant_attachment`], to
/// be verified against its declared `.tgz` integrity.
pub(super) fn prepare_variant_attachment(
    attachment: PendingAttachment,
    (canonical, version, tag): (String, String, String),
    declared: &Declarations,
) -> Result<PreparedAttachment, RegistryError> {
    let dist = attachment_dist(declared, &version, &tag)?;
    Ok(PreparedAttachment {
        attachment,
        canonical,
        version,
        dist,
        kind: AttachmentKind::Variant,
        variant_tag: Some(tag),
    })
}

/// The `dist` a variant attachment is verified against.
fn attachment_dist(
    declared: &Declarations,
    version: &str,
    tag: &str,
) -> Result<Value, RegistryError> {
    declared
        .get(version)
        .and_then(|variants| {
            variants
                .iter()
                .find(|variant| variant.tag == tag)
        })
        .map(|variant| json!({ "integrity": variant.tgz_integrity }))
        .ok_or_else(|| {
            bad_request(format!("attachment for variant {tag:?} of {version} is not declared"))
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
    tag: Option<String>,
    integrity: &Integrity,
    path: &std::path::Path,
) -> Option<VariantTar> {
    tag.map(|tag| VariantTar {
        tag,
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

/// Move each version's stored variants into `dist.pnpr.variants`, with the
/// route each is served at.
pub fn describe_variants(doc: &mut Value, name: &str, public_url: &str) {
    let Some(versions) = doc.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for (version, manifest) in versions.iter_mut() {
        describe_version_variants(manifest, name, version, public_url);
    }
}

/// [`describe_version_variants`] for a version manifest, which names its
/// version.
pub fn describe_manifest_variants(manifest: &mut Value, name: &str, public_url: &str) {
    let Some(version) = manifest
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    describe_version_variants(manifest, name, &version, public_url);
}

/// [`describe_variants`] for one version manifest.
pub fn describe_version_variants(
    manifest: &mut Value,
    name: &str,
    version: &str,
    public_url: &str,
) {
    let Some(manifest) = manifest.as_object_mut() else { return };
    let Some(Value::Array(mut variants)) = manifest.remove(STORED_FIELD) else { return };
    let public_url = public_url.trim_end_matches('/');
    for variant in &mut variants {
        let Some(variant) = variant.as_object_mut() else { continue };
        let tag = variant
            .get("tag")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        variant.insert(
            "tarball".into(),
            Value::String(format!("{public_url}/{name}@{version}/variants/{tag}")),
        );
    }
    let Some(dist) = manifest.get_mut("dist").and_then(Value::as_object_mut) else { return };
    let pnpr = dist
        .entry("pnpr")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(pnpr) = pnpr.as_object_mut() {
        pnpr.insert("variants".into(), Value::Array(variants));
    }
}
