//! Release records, `<base>/<name>/<version>+r<revision>`: the facts about a
//! version's archives and its manifest, which never change once accepted,
//! and the npm-format views from which pnpr's facts are removed.

use super::{
    super::{describe_tar, has_settled_tar_facts, publishing::variants::SHARED_MANIFEST_FIELD},
    DIST_FIELDS, PNPR_FIELDS,
};
use axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use pnpr_storage::DerivedTar;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A version document's address: `<version>+r<revision>` names one release
/// record, which never changes. A bare version or a tag names whatever the
/// registry currently selects.
pub(in super::super) fn record_address(version_or_tag: &str) -> (&str, Option<u64>) {
    version_or_tag
        .rsplit_once("+r")
        .and_then(|(version, revision)| Some((version, parse_revision(revision)?)))
        .map_or((version_or_tag, None), |(version, revision)| (version, Some(revision)))
}

/// A revision number in its canonical form: digits without a leading zero.
fn parse_revision(revision: &str) -> Option<u64> {
    let canonical = !revision.is_empty()
        && revision.bytes().all(|byte| byte.is_ascii_digit())
        && (revision == "0" || !revision.starts_with('0'));
    canonical.then(|| revision.parse().ok())?
}

/// The derived tar metadata a version document is described with, and
/// whether the registry serves tars.
pub(in super::super) type TarFacts<'a> = Option<(&'a BTreeMap<String, DerivedTar>, bool)>;

/// The response for a version's document. Addressed by `revision`, it is
/// that revision's release record. Otherwise it is npm's view of the
/// selected revision, without pnpr's facts.
pub(in super::super) fn version_document_response(
    mut manifest: Value,
    revision: Option<u64>,
    tars: TarFacts<'_>,
) -> Response {
    if let Some(revision) = revision {
        return release_record_response(&manifest, revision, tars);
    }
    if let Some((table, tar_registry)) = tars {
        describe_tar(&mut manifest, table, tar_registry);
    }
    without_pnpr_facts(&mut manifest);
    json_response(&manifest)
}

/// The release record of `revision`. It is cached forever only once pnpr
/// has derived every fact about its archive: until then, a later read could
/// add one.
fn release_record_response(manifest: &Value, revision: u64, tars: TarFacts<'_>) -> Response {
    let Some(mut source) = revision_source(manifest, revision) else {
        return super::super::not_found();
    };
    let settled = tars.is_some_and(|(table, _)| has_settled_tar_facts(&source, table));
    if let Some((table, tar_registry)) = tars {
        describe_tar(&mut source, table, tar_registry);
    }
    // A release record is part of pnpr's own format, so it describes the
    // variants; npm's version document does not.
    super::super::publishing::variants::describe_version_variants(&mut source);
    let response = json_response(&named_record(manifest, &source));
    if settled { immutable(response) } else { response }
}

/// The response for the release record a version was accepted with,
/// `bytes` as the metadata store keeps them. They never change, so they are
/// served as they are and cached forever.
///
/// `tars` is the tar-integrity table of a registry that serves tars: the
/// record is described by its tar's integrity instead of the `.tgz`'s it was
/// accepted with, and is cached forever only once that integrity is known.
/// Those are not the bytes the record was accepted with.
pub(in super::super) fn accepted_record_response(
    bytes: Vec<u8>,
    tars: Option<&BTreeMap<String, DerivedTar>>,
) -> Response {
    let Some(table) = tars else { return immutable(json_bytes_response(bytes)) };
    let mut record: Value = match serde_json::from_slice(&bytes) {
        Ok(record) => record,
        Err(err) => return super::super::RegistryError::Json(err).into_response(),
    };
    let tar_integrity = record
        .pointer("/inputs/archive/integrity")
        .and_then(Value::as_str)
        .and_then(|tgz| table.get(tgz))
        .map(|derived| derived.tar_integrity.clone());
    let Some(fields) = record.as_object_mut() else { return json_response(&record) };
    let Some(integrity) = tar_integrity else {
        fields.remove("integrity");
        return json_response(&record);
    };
    fields.insert("integrity".into(), Value::String(integrity));
    immutable(json_response(&record))
}

/// `response`, marked to be cached forever when it succeeded.
fn immutable(mut response: Response) -> Response {
    if response.status().is_success() {
        response
            .headers_mut()
            .insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
    }
    response
}

fn json_bytes_response(body: Vec<u8>) -> Response {
    super::super::packument_bytes_response(body, "application/json", None)
}

fn json_response(document: &Value) -> Response {
    match serde_json::to_vec(document) {
        Ok(body) => json_bytes_response(body),
        Err(err) => super::super::RegistryError::Json(err).into_response(),
    }
}

/// The version manifest `revision`'s record is made of. A version with a
/// revision history takes every revision's from its entry, so a record's
/// bytes never depend on which revision is selected. A version outside the
/// revision protocol has only its original, revision 0.
fn revision_source(manifest: &Value, revision: u64) -> Option<Value> {
    let Some(history) = manifest.pointer("/dist/revisions") else {
        return (revision == selected_revision(manifest)).then(|| manifest.clone());
    };
    let entry = history
        .as_array()?
        .iter()
        .find(|entry| entry.get("revision").and_then(Value::as_u64) == Some(revision))?;
    let mut fields = match entry.get("manifest") {
        Some(Value::Object(fields)) => fields.clone(),
        // An entry without a manifest of its own is described only by the
        // selected revision's projection.
        _ if revision == selected_revision(manifest) => manifest.as_object()?.clone(),
        _ => return None,
    };
    // The revision marks the integrity as the tar's, which describing it
    // keeps.
    let mut dist = Map::new();
    dist.insert("integrity".into(), entry.get("integrity")?.clone());
    dist.insert("revision".into(), Value::from(revision));
    fields.insert("dist".into(), Value::Object(dist));
    Some(Value::Object(fields))
}

/// The revision a version's manifest is projected from: `0`, the original,
/// unless the registry selected a replacement.
pub(in super::super) fn selected_revision(manifest: &Value) -> u64 {
    manifest
        .pointer("/dist/revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// The release record of `manifest`, named by the version `named` is of.
fn named_record(named: &Value, manifest: &Value) -> Value {
    let mut record = Map::new();
    for field in ["name", "version"] {
        record.insert(
            field.into(),
            named
                .get(field)
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    record.extend(release_record(manifest));
    Value::Object(record)
}

/// A version's release record without its `name` and `version`: pnpr's
/// facts about its archives, and under `manifest` the publisher's
/// abbreviated manifest without `dist` and without what may change after
/// publication (`deprecated`) or is evidence kept apart from the record
/// (`_npmUser`, signatures, attestations).
pub(in super::super) fn release_record(manifest: &Value) -> Map<String, Value> {
    let mut fields = match pnpr_upstream::abbreviate_version(manifest) {
        Value::Object(fields) => fields,
        _ => Map::new(),
    };
    for field in ["name", "version", "deprecated", "_npmUser"] {
        fields.remove(field);
    }
    let dist = match fields.remove("dist") {
        Some(Value::Object(dist)) => dist,
        _ => Map::new(),
    };
    let pnpr = dist.get("pnpr").and_then(Value::as_object);
    if let Some(record) = pnpr.and_then(record_without_any_archive) {
        return record;
    }
    let mut record = Map::new();
    for field in DIST_FIELDS {
        if let Some(value) = dist.get(field) {
            record.insert(field.into(), value.clone());
        }
    }
    for field in PNPR_FIELDS {
        if let Some(value) = pnpr.and_then(|pnpr| pnpr.get(field)) {
            record.insert(field.into(), value.clone());
        }
    }
    record.insert("manifest".into(), Value::Object(fields));
    record
}

/// The record of a version without an `any` archive, from its `dist.pnpr`:
/// its variants and the manifest they share, with no facts of its own, since
/// the archive pnpr generated for npm's documents is not one of its
/// archives. `None` for any other version.
fn record_without_any_archive(pnpr: &Map<String, Value>) -> Option<Map<String, Value>> {
    let shared = pnpr.get(SHARED_MANIFEST_FIELD)?;
    let mut record = Map::new();
    if let Some(variants) = pnpr.get("variants") {
        record.insert("variants".into(), variants.clone());
    }
    record.insert("manifest".into(), shared.clone());
    Some(record)
}

/// Remove pnpr's extensions from a version manifest served in npm's format:
/// pnpr's facts about the archives, the variants it stores with a hosted
/// version, and the revision protocol's marker and history. The selected
/// revision's `tarball` and `integrity` stay, so npm clients install it.
fn without_pnpr_facts(manifest: &mut Value) {
    if let Some(fields) = manifest.as_object_mut() {
        fields.remove(super::super::publishing::variants::STORED_FIELD);
        fields.remove(super::super::publishing::variants::GENERATED_ARCHIVE_FIELD);
    }
    let dist = manifest.get_mut("dist").and_then(Value::as_object_mut);
    if let Some(dist) = dist {
        for field in ["pnpr", "revision", "revisions"] {
            dist.remove(field);
        }
    }
}

/// Remove pnpr's facts from every version of a document served in npm's
/// format.
pub(in super::super) fn without_pnpr_facts_in(doc: &mut Value) {
    let versions = doc.get_mut("versions").and_then(Value::as_object_mut);
    versions
        .into_iter()
        .flat_map(|versions| versions.values_mut())
        .for_each(without_pnpr_facts);
}
