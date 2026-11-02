//! Release records, `<base>/<name>/<version>+r<revision>`: the facts about a
//! version's archives and its manifest, which never change once accepted,
//! and the npm-format views from which pnpr's facts are removed.

use super::{DIST_FIELDS, PNPR_FIELDS};
use axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value};

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

/// The response for a version's document. Addressed by `revision`, it is
/// that revision's release record, cached forever. Otherwise it is npm's
/// view of the selected revision, without pnpr's facts.
pub(in super::super) fn version_document_response(
    mut manifest: Value,
    revision: Option<u64>,
) -> Response {
    let document = match revision {
        Some(revision) => match revision_record(&manifest, revision) {
            Some(record) => record,
            None => return super::super::not_found(),
        },
        None => {
            without_pnpr_facts(&mut manifest);
            manifest
        }
    };
    let body = match serde_json::to_vec(&document) {
        Ok(body) => body,
        Err(err) => return super::super::RegistryError::Json(err).into_response(),
    };
    let mut response = super::super::packument_bytes_response(body, "application/json", None);
    if revision.is_some() {
        response
            .headers_mut()
            .insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
    }
    response
}

/// The release record of `revision` of the version `manifest` describes:
/// the selected revision's from the manifest itself, any other accepted
/// revision's from the version's revision history. Revision 0 of a version
/// outside the revision protocol is its only artifact.
fn revision_record(manifest: &Value, revision: u64) -> Option<Value> {
    if revision == selected_revision(manifest) {
        return Some(named_record(manifest, manifest));
    }
    let entry = manifest
        .pointer("/dist/revisions")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("revision").and_then(Value::as_u64) == Some(revision))?;
    let mut fields = entry
        .get("manifest")?
        .as_object()?
        .clone();
    let mut dist = Map::new();
    dist.insert("integrity".into(), entry.get("integrity")?.clone());
    fields.insert("dist".into(), Value::Object(dist));
    Some(named_record(manifest, &Value::Object(fields)))
}

/// The revision a version's manifest is projected from: `0`, the original,
/// unless the registry selected a replacement.
fn selected_revision(manifest: &Value) -> u64 {
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
pub(super) fn release_record(manifest: &Value) -> Map<String, Value> {
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

/// Remove pnpr's extensions from a version manifest served in npm's format:
/// pnpr's facts about the archives, and the revision protocol's marker and
/// history. The selected revision's `tarball` and `integrity` stay, so npm
/// clients install it.
fn without_pnpr_facts(manifest: &mut Value) {
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
