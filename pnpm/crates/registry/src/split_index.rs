//! pnpr's split packuments (proof of concept). Above a size threshold, the
//! `application/vnd.pnpr.install-v2+json` form lists only the versions a
//! resolution likely picks in `versions`; every other version is an entry of
//! `pnpr.otherVersions` holding what picking reads and what may change after
//! publication: its `deprecated` notice, trust rank, and selected revision,
//! and its publish time if it is after `pnpr.timesSince`. The rest of the
//! version is in its release record, `<registry><name>/<version>+r<revision>`.
//!
//! An entry without a time was published before `pnpr.timesSince`, which is
//! at least 30 days before the document was served, and pnpm accepts a
//! minimum release age of at most 30 days. So such a version is recorded as
//! published at `timesSince`, which every release-age cutoff passes.
//!
//! Where the document is parsed, each entry becomes a version fragment marked
//! `dist.pnpr.details: "external"`, so the picker, the trust policy, and the
//! release-age filter read it like any other version.

use serde::Deserialize;
use serde_json::{Map, Value, json, value::RawValue};

use crate::Package;

#[derive(Deserialize)]
struct SplitDocument {
    pnpr: Option<SplitIndex>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitIndex {
    times_since: Option<String>,
    other_versions: Option<Map<String, Value>>,
}

/// Add the other versions of the split document `body` to `meta`, which was
/// parsed from it.
pub fn add_other_versions(body: &str, meta: &mut Package) -> serde_json::Result<()> {
    if !body.contains(r#""otherVersions""#) {
        return Ok(());
    }
    let document: SplitDocument = serde_json::from_str(body)?;
    let Some(SplitIndex {
        times_since,
        other_versions: Some(entries),
    }) = document.pnpr
    else {
        return Ok(());
    };
    let mut fragments = Vec::with_capacity(entries.len());
    for (version, entry) in entries {
        let published = match entry.get("time") {
            Some(Value::String(published)) => Some(published.clone()),
            Some(_) => None,
            None => times_since.clone(),
        };
        if let Some(published) = published {
            meta.time
                .get_or_insert_default()
                .insert(version.clone(), Value::String(published));
        }
        let fragment = fragment(&meta.name, &version, &entry);
        fragments.push((version, RawValue::from_string(fragment.to_string())?));
    }
    meta.versions.extend_raw(fragments);
    Ok(())
}

/// The version fragment an external entry stands for.
fn fragment(name: &str, version: &str, flags: &Value) -> Value {
    let mut manifest = Map::new();
    manifest.insert("name".into(), json!(name));
    manifest.insert("version".into(), json!(version));
    if let Some(deprecated) = flags.get("deprecated") {
        manifest.insert("deprecated".into(), deprecated.clone());
    }
    let mut dist = Map::new();
    match flags.get("trust").and_then(Value::as_str) {
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
    if let Some(revision) = flags.get("revision") {
        dist.insert("revision".into(), revision.clone());
    }
    dist.insert("pnpr".into(), json!({ "details": "external" }));
    manifest.insert("dist".into(), Value::Object(dist));
    Value::Object(manifest)
}

fn provenance() -> Value {
    json!({ "provenance": { "predicateType": "https://slsa.dev/provenance/v1" } })
}

#[cfg(test)]
mod tests;
