//! The release record a published version is accepted with
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md)).
//!
//! A record is the version's facts about its archives and its manifest, as
//! its record route already served them, with the core fields every record
//! has in front: its `schema`, its `purl`, its `revision`, the
//! `interpretation` profile its manifest was derived under, and the
//! `inputs` it was derived from. It is built once, when the version is
//! accepted, and its bytes never change: the metadata store keeps them and
//! the record route serves them as they are.
//!
//! The record's `integrity` is what the record route has always served for
//! the version: for a version outside the revision protocol, its `.tgz`'s
//! as published. Its tar's digest is in the tar-integrity table, and a
//! registry that serves tars describes the version with it.

use super::{
    attachments::PublishedTar,
    variants::{self, PublishedVariant, STORED_FIELD},
};
use pnpm_release_record::{Purl, RECORD_SCHEMA};
use pnpm_tarball::PublicationArchive;
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{DerivedTar, metadata::ReleaseRecords};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The digests a record's manifest was derived from: the `.tgz` as
/// published, by the digest npm clients and npm provenance use, its
/// `package.json`, and its `npm-shrinkwrap.json`, each by its path in the
/// archive.
pub(super) fn archive_inputs(tgz_integrity: Option<&str>, archive: &PublicationArchive) -> Value {
    let shrinkwrap = archive.shrinkwrap_integrity
        .as_ref()
        .zip(archive.shrinkwrap_path.as_deref())
        .map_or(
            Value::Null,
            |(integrity, path)| json!({ "path": path, "integrity": integrity.to_string() }),
        );
    json!({
        "archive": { "kind": "tgz", "integrity": tgz_integrity },
        "packageJson": {
            "path": archive.manifest_path,
            "integrity": archive.manifest_integrity.to_string(),
        },
        "shrinkwrap": shrinkwrap,
    })
}

/// The release record of every version whose `any` archive `tars`
/// publishes, from its verified manifest in `merged` and the facts derived
/// from its archives.
pub(super) fn accepted_records(
    name: &CanonicalPackageName,
    merged: &Value,
    tars: &[PublishedTar],
    variants: &[PublishedVariant],
) -> Result<ReleaseRecords, RegistryError> {
    tars.iter()
        .filter(|tar| tar.variant.is_none())
        .map(|tar| {
            let manifest = merged
                .get("versions")
                .and_then(|versions| versions.get(&tar.version))
                .ok_or_else(|| missing(name, &tar.version, "version metadata"))?;
            let record = accepted_record(name, manifest, tar, variants)?;
            Ok((tar.version.clone(), serde_json::to_vec(&record)?))
        })
        .collect()
}

fn accepted_record(
    name: &CanonicalPackageName,
    manifest: &Value,
    tar: &PublishedTar,
    variants: &[PublishedVariant],
) -> Result<Value, RegistryError> {
    let version = tar.version.as_str();
    let profile = manifest
        .pointer(&format!("/{}/profile", pnpr_storage::metadata::ARCHIVE_MANIFEST_FIELD))
        .and_then(Value::as_str)
        .ok_or_else(|| missing(name, version, "an interpretation profile"))?;
    let purl = version_purl(name, version)?;
    let mut source = manifest.clone();
    describe_archive(&mut source, tar);
    add_variant_sources(&mut source, (name, version), variants)?;
    variants::describe_version_variants(&mut source);
    let mut record = Map::new();
    record.insert("schema".into(), Value::from(RECORD_SCHEMA));
    record.insert("purl".into(), Value::from(purl.as_str()));
    record.insert("name".into(), Value::from(name.as_str()));
    record.insert("version".into(), Value::from(version));
    record.insert("revision".into(), Value::from(0));
    record.insert("interpretation".into(), Value::from(profile));
    // The archive pnpr generates for a version published without one is not
    // an input of the version.
    if !tar.generated {
        record.insert("inputs".into(), archive_inputs(tar.tgz_integrity.as_deref(), &tar.archive));
    }
    record.extend(super::super::split_packuments::release_record(&source));
    Ok(Value::Object(record))
}

/// Describe the version's `any` archive with the facts derived from it,
/// as the record route describes it from the tar-integrity table.
fn describe_archive(manifest: &mut Value, tar: &PublishedTar) {
    let Some(tgz_integrity) = &tar.tgz_integrity else { return };
    let inspection = &tar.inspection;
    let derived = DerivedTar::new(
        tar.integrity.to_string(),
        inspection.requires_build,
        inspection.file_count,
        inspection.unpacked_size,
    )
    .with_bins(inspection.bins.clone());
    let table = BTreeMap::from([(tgz_integrity.clone(), derived)]);
    super::super::describe_tar(manifest, &table, false);
}

/// Put each variant's `purl` and inputs on its stored entry, which the
/// record lists.
fn add_variant_sources(
    manifest: &mut Value,
    (name, version): (&CanonicalPackageName, &str),
    published: &[PublishedVariant],
) -> Result<(), RegistryError> {
    let Some(Value::Array(stored)) = manifest.get_mut(STORED_FIELD) else { return Ok(()) };
    for entry in stored.iter_mut().filter_map(Value::as_object_mut) {
        let tag = entry.get("tag").and_then(Value::as_str);
        let Some(variant) = published
            .iter()
            .find(|variant| variant.version == version && Some(variant.tar.tag.as_str()) == tag)
        else {
            continue;
        };
        let purl = variant_purl(name, version, &variant.tar.tag)?;
        let mut sourced = Map::new();
        sourced.insert("purl".into(), Value::from(purl.as_str()));
        sourced.insert("inputs".into(), variant.tar.inputs.clone());
        sourced.append(entry);
        *entry = sourced;
    }
    Ok(())
}

/// The PURL a variant's archive is named by, in its release record and in
/// the version's provenance statement.
pub(super) fn variant_purl(
    name: &CanonicalPackageName,
    version: &str,
    tag: &str,
) -> Result<Purl, RegistryError> {
    Purl::npm_variant(name.as_str(), version, tag).map_err(|err| no_purl(name, version, &err))
}

/// The PURL a version's `any` archive is named by.
pub(super) fn version_purl(
    name: &CanonicalPackageName,
    version: &str,
) -> Result<Purl, RegistryError> {
    Purl::npm(name.as_str(), version).map_err(|err| no_purl(name, version, &err))
}

fn no_purl(
    name: &CanonicalPackageName,
    version: &str,
    err: &pnpm_release_record::PurlError,
) -> RegistryError {
    RegistryError::BadRequest {
        reason: format!("{}@{version} has no package URL: {err}", name.as_str()),
    }
}

fn missing(name: &CanonicalPackageName, version: &str, what: &str) -> RegistryError {
    RegistryError::Internal {
        reason: format!(
            "{}@{version} has no {what} to build its release record from",
            name.as_str(),
        ),
    }
}
