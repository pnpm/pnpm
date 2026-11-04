//! Publishing a version with its platform variants to pnpr
//! ([platform variants](https://github.com/pnpm/rfcs/blob/1a9891a2073f7c03a89fc6756eb78881853fa218/pnpr/text/0000-platform-variants.md#sending-a-version-with-variants)).
//!
//! Every archive is staged by its integrity before the document is sent, and
//! the document references each one: the `any` archive as the version's
//! `<name>-<version>.tgz`, each platform variant as
//! `<name>-<version>~<tag>.tgz`. A version without an `any` archive has no
//! `dist` and no plain attachment; pnpr generates the archive npm's
//! documents point to.
//!
//! An archive another machine staged, such as the job of a CI matrix that
//! built it, is referenced by the integrity it was staged under, once a
//! `HEAD` shows the registry still holds it for the caller.

use super::{PackedPkg, PublishPackedPkgError, Value, clean_version, registry_for_display};
use crate::{
    provenance_gen::ProvenanceSubject,
    registry_config_keys::NormalizedRegistryUrl,
    staged_archives::{StagedArchive, staged_length},
};
use futures_util::future::try_join_all;
use pnpm_network::ThrottledClient;
use pnpm_release_record::{Purl, PurlError};
use std::collections::{HashMap, HashSet};

/// One platform variant's archive, published with its version.
#[derive(Debug, Clone)]
pub struct PackedVariant {
    /// The variant's tag, which names its attachment.
    pub tag: String,
    pub archive: VariantArchive,
}

/// Where an archive published with its version comes from.
#[derive(Debug, Clone)]
pub enum VariantArchive {
    /// Packed by this publish, which stages it.
    Packed(Vec<u8>),
    /// Staged before this publish, such as by a job of a CI matrix.
    Staged(StagedReference),
}

/// An archive staged before the publish, named by its integrity.
#[derive(Debug, Clone)]
pub struct StagedReference {
    pub integrity: String,
    /// The archive as an error names it: its entry's targets.
    pub label: String,
}

impl VariantArchive {
    pub(super) fn integrity(&self) -> String {
        match self {
            VariantArchive::Packed(bytes) => sri_sha512(bytes),
            VariantArchive::Staged(staged) => staged.integrity.clone(),
        }
    }
}

/// The sizes of the staged archives a publish references, by integrity, as
/// the registry reports them.
pub(crate) type StagedLengths = HashMap<String, u64>;

/// Refuse to publish variants to a registry that is not declared with
/// `serverType: pnpr`: a version published without its variants cannot get
/// them later.
pub(crate) fn check_registry(
    pkg: &PackedPkg<'_>,
    pnpr_registries: &HashSet<String>,
    registry: &NormalizedRegistryUrl,
) -> Result<(), PublishPackedPkgError> {
    let has_variants = pkg.publishes_staged_archives();
    if !has_variants || pnpr_registries.contains(registry.as_str()) {
        return Ok(());
    }
    Err(PublishPackedPkgError::VariantsNeedPnpr { registry: registry_for_display(registry) })
}

/// The archives this publish packed, to stage before the document that
/// references them: the `any` archive, whose integrity is `any_integrity`,
/// and every packed variant.
pub(crate) fn staged_archives<'a>(
    pkg: &PackedPkg<'a>,
    any_integrity: &str,
) -> Vec<StagedArchive<'a>> {
    let any = (!pkg.tarball_data.is_empty() && pkg.any_staged.is_none()).then(|| StagedArchive {
        integrity: any_integrity.to_string(),
        bytes: pkg.tarball_data,
    });
    let packed = pkg.variants
        .iter()
        .filter_map(|variant| match &variant.archive {
            VariantArchive::Packed(bytes) => {
                Some(StagedArchive { integrity: sri_sha512(bytes), bytes })
            }
            VariantArchive::Staged(_) => None,
        });
    any.into_iter().chain(packed).collect()
}

/// The size of every archive `pkg` references as staged before the publish,
/// asked with `HEAD`, all at once. One the registry does not hold for the
/// caller fails the publish before its document is sent, naming the archive.
pub(crate) async fn staged_lengths(
    pkg: &PackedPkg<'_>,
    client: &ThrottledClient,
    registry: &NormalizedRegistryUrl,
    authorization: Option<&str>,
) -> Result<StagedLengths, PublishPackedPkgError> {
    let staged = pkg.any_staged
        .into_iter()
        .chain(
            pkg.variants
                .iter()
                .filter_map(|variant| match &variant.archive {
                    VariantArchive::Staged(staged) => Some(staged),
                    VariantArchive::Packed(_) => None,
                }),
        );
    let lengths = try_join_all(staged.map(|reference| async move {
        let length = staged_length(client, registry, authorization, &reference.integrity)
            .await
            .map_err(PublishPackedPkgError::Staging)?
            .ok_or_else(|| PublishPackedPkgError::NotStaged {
                label: reference.label.clone(),
                integrity: reference.integrity.clone(),
                registry: registry_for_display(registry),
            })?;
        Ok::<_, PublishPackedPkgError>((reference.integrity.clone(), length))
    }))
    .await?;
    Ok(lengths.into_iter().collect())
}

/// Make `document` reference the staged archives instead of carrying them:
/// the `any` archive's attachment names its integrity, each variant gets an
/// attachment named after it, and a version without an `any` archive loses
/// its `dist` and plain attachment. An `any` archive staged before the
/// publish is described by its `dist.integrity` alone. `version` is the
/// manifest's, which may carry build metadata.
pub(crate) fn reference_staged_archives(
    document: &mut Value,
    pkg: &PackedPkg<'_>,
    (version, lengths): (&str, &StagedLengths),
) {
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
    attachments.shift_remove(&any_name);
    let any = any_archive(pkg, lengths);
    if let Some((integrity, length)) = &any {
        attachments.insert(any_name.clone(), staged_attachment(integrity, *length));
    }
    let stem = any_name.strip_suffix(".tgz").unwrap_or(&any_name);
    attachments.extend(variant_attachments(pkg, stem, lengths));
    // The document keys the version as npm does, without build metadata.
    let key = clean_version(version).unwrap_or_else(|_| version.to_string());
    if let Some(manifest) = document
        .pointer_mut("/versions")
        .and_then(|versions| versions.get_mut(&key))
        .and_then(Value::as_object_mut)
    {
        describe_any_archive(manifest, pkg);
    }
}

/// Each variant's attachment, `<stem>~<tag>.tgz`, referencing its staged
/// archive.
fn variant_attachments<'a>(
    pkg: &'a PackedPkg<'_>,
    stem: &'a str,
    lengths: &'a StagedLengths,
) -> impl Iterator<Item = (String, Value)> + 'a {
    pkg.variants
        .iter()
        .map(move |variant| {
            let integrity = variant.archive.integrity();
            let length = match &variant.archive {
                VariantArchive::Packed(bytes) => bytes.len() as u64,
                VariantArchive::Staged(_) => lengths
                    .get(&integrity)
                    .copied()
                    .unwrap_or_default(),
            };
            (format!("{stem}~{}.tgz", variant.tag), staged_attachment(&integrity, length))
        })
}

/// The integrity and size of the version's `any` archive, when it has one.
fn any_archive(pkg: &PackedPkg<'_>, lengths: &StagedLengths) -> Option<(String, u64)> {
    match pkg.any_staged {
        Some(staged) => Some((
            staged.integrity.clone(),
            lengths
                .get(&staged.integrity)
                .copied()
                .unwrap_or_default(),
        )),
        None if pkg.has_any_archive() => {
            Some((sri_sha512(pkg.tarball_data), pkg.tarball_data.len() as u64))
        }
        None => None,
    }
}

/// Describe the version by `pkg`'s `any` archive: one another machine
/// staged gives it only its `dist.integrity`, none leaves it no `dist`, and
/// one this publish packed keeps the `dist` the document was built with.
fn describe_any_archive(manifest: &mut serde_json::Map<String, Value>, pkg: &PackedPkg<'_>) {
    match pkg.any_staged {
        Some(staged) => {
            manifest.insert("dist".into(), serde_json::json!({ "integrity": staged.integrity }));
        }
        None if !pkg.has_any_archive() => {
            manifest.shift_remove("dist");
        }
        None => {}
    }
}

fn staged_attachment(integrity: &str, length: u64) -> Value {
    serde_json::json!({
        "content_type": "application/octet-stream",
        "integrity": integrity,
        "length": length,
    })
}

/// The subjects of the provenance statement of a version with variants:
/// each archive it publishes, by its PURL (a variant's with the `variant`
/// qualifier) and its `.tgz` SHA-512. An `any` archive pnpr generates for a
/// version without one is no subject.
pub(super) fn provenance_subjects(
    pkg: &PackedPkg<'_>,
    (name, version): (&str, &str),
) -> Result<Vec<ProvenanceSubject>, PublishPackedPkgError> {
    let purl_error =
        |error: PurlError| PublishPackedPkgError::Purl { name: name.to_string(), error };
    let any = match pkg.any_staged {
        Some(staged) => Some(staged.integrity.clone()),
        None => pkg.has_any_archive().then(|| sri_sha512(pkg.tarball_data)),
    };
    let mut subjects = Vec::with_capacity(pkg.variants.len() + 1);
    if let Some(integrity) = any {
        let purl = Purl::npm(name, version).map_err(purl_error)?;
        subjects.push(subject(&purl, &integrity)?);
    }
    for variant in pkg.variants {
        let purl = Purl::npm_variant(name, version, &variant.tag).map_err(purl_error)?;
        subjects.push(subject(&purl, &variant.archive.integrity())?);
    }
    Ok(subjects)
}

fn subject(purl: &Purl, integrity: &str) -> Result<ProvenanceSubject, PublishPackedPkgError> {
    let sha512_hex = sha512_hex(integrity)
        .ok_or_else(|| PublishPackedPkgError::NoSha512 { integrity: integrity.to_string() })?;
    Ok(ProvenanceSubject { name: purl.as_str().to_string(), sha512_hex })
}

/// The lowercase hex SHA-512 an SRI `integrity` carries.
fn sha512_hex(integrity: &str) -> Option<String> {
    use base64::Engine as _;
    use std::fmt::Write as _;
    let integrity: ssri::Integrity = integrity.parse().ok()?;
    let hash = integrity.hashes
        .iter()
        .find(|hash| hash.algorithm == ssri::Algorithm::Sha512)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(&hash.digest).ok()?;
    Some(
        bytes
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }),
    )
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
