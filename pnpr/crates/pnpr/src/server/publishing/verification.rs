//! Publish-time manifest verification: the version metadata a publisher
//! submits must agree with the `package.json` inside its archive, and what is
//! stored and served for the version is derived from the archive.

use pnpm_package_manifest::{
    ArchiveFacts, INSTALL_MANIFEST_PROFILE, InstallManifest, ManifestComparison, NODE_GYP_REBUILD,
    parse_manifest_bytes,
};
use pnpm_tarball::{PublicationArchive, read_publication_archive};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::metadata::ARCHIVE_MANIFEST_FIELD;
use serde_json::{Map, Value, json};
use std::{io::Read, path::Path};

use super::attachments::PublishedTar;

/// Read the `.tgz` written to `tgz_path` for the manifest an installer would
/// select, decoding no more than any `.tgz` may decode to.
pub(super) fn read_archive(
    tgz_path: &Path,
    filename: &str,
) -> Result<PublicationArchive, RegistryError> {
    let tgz = std::io::BufReader::new(std::fs::File::open(tgz_path)?);
    let limit = super::super::tar_decode_limit(None);
    let tar = flate2::read::MultiGzDecoder::new(tgz).take(limit);
    read_publication_archive(tar)
        .map_err(|err| RegistryError::InvalidAttachment {
            filename: filename.to_string(),
            reason: err.to_string(),
        })
}

/// Verify every version `tars` publishes against the version metadata
/// submitted in `document`, refusing a tarball whose version has none, and
/// replace that metadata's install-critical fields with the ones derived from
/// the archive. Returns a notice for each difference the profile accepts, for
/// the publisher.
pub(super) fn verify_published_manifests(
    name: &CanonicalPackageName,
    document: &mut Value,
    tars: &[PublishedTar],
) -> Result<Vec<String>, RegistryError> {
    let mut notices = Vec::new();
    for tar in tars {
        let Some(version) = document
            .get_mut("versions")
            .and_then(|versions| versions.get_mut(&tar.version))
            .and_then(Value::as_object_mut)
        else {
            // Staging already refuses a tarball with no version metadata;
            // refusing it here too keeps an unverified archive out.
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "cannot publish the tarball of {}@{} without version metadata",
                    name.as_str(),
                    tar.version,
                ),
            });
        };
        notices.extend(verify_version(name, &tar.version, version, &tar.archive)?);
    }
    Ok(notices)
}

fn verify_version(
    name: &CanonicalPackageName,
    version_key: &str,
    version: &mut Map<String, Value>,
    archive: &PublicationArchive,
) -> Result<Option<String>, RegistryError> {
    let manifest = parse_archive_manifest(name, version_key, archive)?;
    let facts = ArchiveFacts {
        binding_gyp: archive.binding_gyp,
        has_shrinkwrap: archive.shrinkwrap_integrity.is_some(),
        files: &archive.files,
    };
    let derived = InstallManifest::derive(&manifest, facts);
    let submitted = derive_submitted(version, facts);
    let mut comparison = submitted.compare(&derived);
    check_slot(name, version_key, &derived, &mut comparison);
    if !comparison.is_accepted() {
        return Err(RegistryError::ManifestMismatch {
            package: name.as_str().to_string(),
            version: version_key.to_string(),
            profile: INSTALL_MANIFEST_PROFILE,
            fields: comparison.mismatched.join(", "),
        });
    }
    derived.write_to_version(version);
    version.insert(ARCHIVE_MANIFEST_FIELD.to_string(), archive_manifest_record(archive));
    Ok(tolerated_notice(name, version_key, &comparison))
}

/// Derive the install-critical fields of the submitted `version`, which is
/// left as it was.
fn derive_submitted(version: &mut Map<String, Value>, facts: ArchiveFacts<'_>) -> InstallManifest {
    let submitted = Value::Object(std::mem::take(version));
    let derived = InstallManifest::derive(&submitted, facts);
    if let Value::Object(submitted) = submitted {
        *version = submitted;
    }
    derived
}

fn parse_archive_manifest(
    name: &CanonicalPackageName,
    version_key: &str,
    archive: &PublicationArchive,
) -> Result<Value, RegistryError> {
    parse_manifest_bytes(&archive.manifest)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: format!(
                "the package.json in the archive of {}@{version_key} is not a JSON object",
                name.as_str(),
            ),
        })
}

/// The archive must describe the slot it is published into: the package the
/// URL names, and the version its attachment names. The derived version has
/// no build metadata, as npm publishes it, so a slot with build metadata never
/// matches: a publisher can never claim the `+r<N>` that pnpm reserves for
/// registry revisions.
fn check_slot(
    name: &CanonicalPackageName,
    version_key: &str,
    derived: &InstallManifest,
    comparison: &mut ManifestComparison,
) {
    let fields = derived.fields();
    let field_is =
        |field: &str, expected: &str| fields.get(field).and_then(Value::as_str) == Some(expected);
    for (field, expected) in [("name", name.as_str()), ("version", version_key)] {
        if !field_is(field, expected)
            && !comparison.mismatched
                .iter()
                .any(|seen| seen == field)
        {
            comparison.mismatched.push(field.to_string());
        }
    }
}

/// Describe `version`, published without an `any` archive, by the archive
/// pnpr generated for npm's documents: its install-critical fields are the
/// generated archive's, and `dist` names that archive, at `tarball`.
pub(super) fn describe_generated_archive(
    version: &mut Map<String, Value>,
    archive: &PublicationArchive,
    mut dist: Value,
    tarball: &str,
) {
    let manifest = parse_manifest_bytes(&archive.manifest).unwrap_or_default();
    let facts = ArchiveFacts {
        binding_gyp: archive.binding_gyp,
        has_shrinkwrap: archive.shrinkwrap_integrity.is_some(),
        files: &archive.files,
    };
    InstallManifest::derive(&manifest, facts).write_to_version(version);
    version.insert(ARCHIVE_MANIFEST_FIELD.to_string(), archive_manifest_record(archive));
    dist["tarball"] = Value::String(tarball.to_string());
    if let Some(attestations) = version
        .get("dist")
        .and_then(|old| old.get("attestations"))
    {
        dist["attestations"] = attestations.clone();
    }
    version.insert("dist".to_string(), dist);
}

fn archive_manifest_record(archive: &PublicationArchive) -> Value {
    let mut record = json!({
        "profile": INSTALL_MANIFEST_PROFILE,
        "packageJson": archive.manifest_integrity.to_string(),
    });
    if let Some(shrinkwrap) = &archive.shrinkwrap_integrity {
        record["npmShrinkwrap"] = Value::String(shrinkwrap.to_string());
    }
    record
}

fn tolerated_notice(
    name: &CanonicalPackageName,
    version_key: &str,
    comparison: &ManifestComparison,
) -> Option<String> {
    if comparison.tolerated.is_empty() {
        return None;
    }
    Some(format!(
        "{}@{version_key}: the submitted scripts.install {NODE_GYP_REBUILD:?} is not served: the \
         archive has no root binding.gyp for it to build, or opts out with gypfile: false",
        name.as_str(),
    ))
}
