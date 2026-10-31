//! pnpr's package document, `application/vnd.pnpr.install-v2+json`
//! (proof of concept). Every version has an entry with what picking it reads
//! and what may change after publication: its `deprecated` notice, `trust`
//! rank, selected `revision`, and its publish `time` if it is after
//! `timesSince`. An entry with a `manifest` is complete; for any other, the
//! manifest is the release record `<registry><name>/<version>+r<revision>`.
//!
//! [`parse_install_v2`] turns the document into a [`Package`] where it is
//! parsed, with a version marked `dist.pnpr.details: "external"` for every
//! entry without a manifest, so the picker, the trust policy, the
//! release-age filter, and the metadata cache read it like any other.
//!
//! An entry without a time was published before `timesSince`, which is at
//! least 30 days before the document was served, and pnpm accepts a minimum
//! release age of at most 30 days. So such a version is recorded as
//! published at `timesSince`, which every release-age cutoff passes. A `null`
//! time is unknown, and recorded as missing.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::Package;

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
        versions.insert(version.clone(), fragment(&document.name, version, entry));
    }
    let mut package = Map::new();
    package.insert("name".into(), Value::String(document.name));
    package.insert("dist-tags".into(), Value::Object(document.dist_tags));
    if let Some(modified) = document.modified {
        package.insert("modified".into(), modified);
    }
    package.insert("time".into(), Value::Object(time));
    package.insert("versions".into(), Value::Object(versions));
    // Versions are kept as raw fragments, which only a string parse makes.
    serde_json::from_str(&Value::Object(package).to_string())
}

/// The version fragment an entry stands for: its manifest when the document
/// has it, else one marked `dist.pnpr.details: "external"`, with the entry's
/// deprecation, trust rank, and revision in npm's fields.
fn fragment(name: &str, version: &str, entry: &Value) -> Value {
    let inline = entry.get("manifest").and_then(Value::as_object);
    let mut manifest = inline.cloned().unwrap_or_default();
    let mut dist = match manifest.remove("dist") {
        Some(Value::Object(dist)) => dist,
        _ => Map::new(),
    };
    manifest.insert("name".into(), json!(name));
    manifest.insert("version".into(), json!(version));
    if let Some(deprecated) = entry.get("deprecated") {
        manifest.insert("deprecated".into(), deprecated.clone());
    }
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
    if let Some(revision) = entry.get("revision") {
        dist.insert("revision".into(), revision.clone());
    }
    if inline.is_none() {
        dist.insert("pnpr".into(), json!({ "details": "external" }));
    }
    manifest.insert("dist".into(), Value::Object(dist));
    Value::Object(manifest)
}

fn provenance() -> Value {
    json!({ "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } })
}

#[cfg(test)]
mod tests;
