//! pnpr's own package document, `application/vnd.pnpr.install-v2+json`
//! ([split packuments](https://github.com/pnpm/rfcs/blob/057a5cb08f430a1824c082ed525bea13520bb4dd/pnpr/text/0000-split-packuments.md)).
//!
//! Every version has an entry with what picking it reads and what may change
//! after publication: its `deprecated` notice, `trust` rank, selected
//! `revision`, and its publish `time` when it is after `timesSince`. An entry
//! with a `manifest` is complete: beside the manifest, it has pnpr's facts
//! about the archives (`integrity`, `fileCount`, `unpackedSize`,
//! `requiresBuild`, `bins`, `variants`). Any other entry's are in its release record,
//! `<registry><name>/<version>+r<revision>`.
//!
//! [`parse_install_v2`] turns the document into a [`Package`] where it is
//! parsed, so the picker, the trust policy, the release-age filter, and the
//! metadata cache read it like any other. Every version carries
//! [`InstallV2Entry`] in `dist.pnpr.installV2`, and the trust rank becomes
//! the npm fields the trust policy reads.
//!
//! An entry without a time was published before `timesSince`, which is at
//! least 30 days before the package's last publish, and pnpm accepts a
//! minimum release age of at most 30 days from this format. So such a
//! version is recorded as published at `timesSince`, which every supported
//! release-age cutoff passes. A `null` time is unknown, and recorded as
//! missing.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{InstallV2Entry, Package, PackageVersion};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallV2 {
    name: String,
    #[serde(default)]
    dist_tags: Map<String, Value>,
    modified: Option<Value>,
    times_since: Option<String>,
    #[serde(default)]
    versions: Map<String, Value>,
}

/// The [`Package`] pnpr's document `body` describes.
pub fn parse_install_v2(body: &str) -> serde_json::Result<Package> {
    let document: InstallV2 = serde_json::from_str(body)?;
    let mut time = Map::new();
    let mut versions = Map::new();
    for (version, entry) in &document.versions {
        let published = match entry.get("time") {
            Some(Value::String(published)) => Some(published.clone()),
            Some(_) => None,
            None => document.times_since.clone(),
        };
        if let Some(published) = published {
            time.insert(version.clone(), Value::String(published));
        }
        versions.insert(version.clone(), npm_version(&document.name, version, entry));
    }
    let mut package = Map::new();
    package.insert("name".into(), Value::String(document.name));
    package.insert("dist-tags".into(), Value::Object(document.dist_tags));
    if let Some(modified) = document.modified {
        package.insert("modified".into(), modified);
    }
    package.insert("time".into(), Value::Object(time));
    package.insert("versions".into(), Value::Object(versions));
    // `Package` keeps each version as a raw JSON fragment, which only a parse
    // from a string produces.
    serde_json::from_str(&Value::Object(package).to_string())
}

/// The [`PackageVersion`] a release record, fetched from
/// `<registry><name>/<version>+r<revision>`, describes. A record without a
/// `manifest` describes nothing, and is an error.
pub fn parse_release_record(body: &[u8]) -> serde_json::Result<PackageVersion> {
    let record: Value = serde_json::from_slice(body)?;
    if !record.get("manifest").is_some_and(Value::is_object) {
        return Err(serde::de::Error::missing_field("manifest"));
    }
    let field = |name: &str| {
        record
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    serde_json::from_value(npm_version(field("name"), field("version"), &record))
}

/// pnpr's facts about the archives, which pnpm keeps in `dist` and
/// `dist.pnpr`.
const DIST_FIELDS: [&str; 3] = ["integrity", "fileCount", "unpackedSize"];
const PNPR_FIELDS: [&str; 3] = ["requiresBuild", "bins", "variants"];

/// The version an entry or release record stands for, in npm's shape, with
/// the deprecation, trust rank, and revision in npm's fields.
fn npm_version(name: &str, version: &str, entry: &Value) -> Value {
    let inline = entry.get("manifest").and_then(Value::as_object);
    let mut manifest = inline.cloned().unwrap_or_default();
    let mut dist = Map::new();
    let mut pnpr = Map::new();
    for (fields, target) in [(&DIST_FIELDS[..], &mut dist), (&PNPR_FIELDS[..], &mut pnpr)] {
        for &field in fields {
            if let Some(value) = entry.get(field) {
                target.insert(field.into(), value.clone());
            }
        }
    }
    manifest.insert("name".into(), json!(name));
    manifest.insert("version".into(), json!(version));
    if let Some(deprecated) = entry.get("deprecated") {
        manifest.insert("deprecated".into(), deprecated.clone());
    }
    add_trust_evidence(entry, &mut manifest, &mut dist);
    if let Some(revision) = entry.get("revision") {
        dist.insert("revision".into(), revision.clone());
    }
    let listed = if inline.is_some() { InstallV2Entry::Complete } else { InstallV2Entry::External };
    pnpr.insert("installV2".into(), json!(listed));
    dist.insert("pnpr".into(), Value::Object(pnpr));
    manifest.insert("dist".into(), Value::Object(dist));
    Value::Object(manifest)
}

/// The npm fields the trust policy reads, for the entry's `trust` rank. The
/// rank stands in for the evidence, which pnpr's format does not carry.
fn add_trust_evidence(
    entry: &Value,
    manifest: &mut Map<String, Value>,
    dist: &mut Map<String, Value>,
) {
    match entry.get("trust").and_then(Value::as_str) {
        Some("stagedPublish") => {
            manifest.insert("_npmUser".into(), json!({ "approver": true }));
        }
        Some("trustedPublisher") => {
            manifest.insert("_npmUser".into(), json!({ "trustedPublisher": true }));
            dist.insert("attestations".into(), provenance());
        }
        Some("provenance") => {
            dist.insert("attestations".into(), provenance());
        }
        _ => {}
    }
}

fn provenance() -> Value {
    json!({ "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } })
}

#[cfg(test)]
mod tests;
